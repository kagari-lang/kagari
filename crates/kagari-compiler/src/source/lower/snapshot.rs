//! Candidate-only assembly: retained bodies and bridge recipes are never regenerated.

use crate::source::lower::{
    LoweredModule, MirLoweringError, ModuleLowering, ModuleLoweringInputs, host,
    instances::MirLoweringOptions, verification_error,
};
use kagari_common::identity::metadata::DefinitionMetadata;
use kagari_contract::{host as module_host, types::identity};
use kagari_mir::{function::MirModule, ids::InstanceId, verify::verify_mir};
use std::iter;

impl ModuleLowering {
    /// Copy unoptimized products and independently verify a fresh module candidate.
    ///
    /// Call only after successful processing and after rechecking all module-prefix
    /// allowances. Optimization may consume this snapshot, never the retained
    /// products. Assembly can fail validation/cancellation but does not alter state
    /// or consume lowering allowance.
    pub fn snapshot(
        &self,
        inputs: &ModuleLoweringInputs<'_>,
        available: &MirLoweringOptions,
    ) -> Result<LoweredModule, MirLoweringError> {
        self.check_budget(available)?;
        if !self.ready
            || self.functions.len() != self.instances.instances.len()
            || self.bridges.processed != self.instances.interface_bridges.len()
            || !self.instances.layout_roots.is_empty()
        {
            return Err(MirLoweringError::MissingBinding(
                "unfinished module lowering",
            ));
        }
        let module = *inputs
            .modules_by_handle
            .get(&self.owner)
            .ok_or(MirLoweringError::MissingBinding("lowering source module"))?;
        let products = self.products();
        let mut functions = products.functions.to_vec();
        for bridge in products.bridge_functions {
            available
                .cancel
                .check()
                .map_err(|_| MirLoweringError::Cancelled)?;
            let mut bridge = bridge.clone();
            bridge.id = InstanceId::new(functions.len());
            functions.push(bridge);
        }
        let mut host_types = self.instances.host_types.clone();
        host_types.extend(
            module_host::references(
                &products.abi.public_items,
                products.structures,
                products.enumerations,
                &available.cancel,
            )
            .map_err(|_| MirLoweringError::Cancelled)?,
        );
        host_types.extend(
            module_host::type_references(
                products
                    .abi
                    .native_declarations
                    .iter()
                    .flat_map(|declaration| {
                        declaration
                            .function
                            .params
                            .iter()
                            .map(|param| &param.ty)
                            .chain(iter::once(&declaration.function.return_type))
                    })
                    .chain(products.native_targets.iter().flat_map(|import| {
                        import
                            .signature
                            .params
                            .iter()
                            .chain(iter::once(&import.signature.result))
                    }))
                    .chain(functions.iter().flat_map(|function| {
                        function
                            .semantic
                            .params
                            .values()
                            .chain(function.semantic.result.iter())
                            .chain(function.semantic.locals.values())
                            .chain(function.semantic.registers.values())
                    })),
                &available.cancel,
            )
            .map_err(|_| MirLoweringError::Cancelled)?,
        );
        let external_types = host::collect(
            module,
            inputs.modules,
            host_types,
            &functions,
            &available.cancel,
            &inputs.context.snapshot(),
        )?;
        let mir = MirModule {
            native_targets: products.native_targets.to_vec(),
            interface_instances: products.interface_instances.to_vec(),
            external_types,
            dependencies: module
                .names
                .imports
                .dependencies
                .iter()
                .map(|handle| {
                    module
                        .names
                        .catalog
                        .definitions()
                        .module_identity(*handle)
                        .expect("checked dependency")
                        .clone()
                })
                .collect(),
            structures: products.structures.to_vec(),
            enumerations: products.enumerations.to_vec(),
            identity: module.lowered.source.module_identity().clone(),
            source_name: module.lowered.source.name().to_owned(),
            module_slots: self.instances.static_slots.clone(),
            abi: products.abi.clone(),
            functions,
        };
        identity::collect(
            &mir.identity,
            &mir.abi.public_items,
            &mir.abi.trait_contracts,
            inputs.context,
            &available.cancel,
        )?;
        let metadata =
            DefinitionMetadata::checked(inputs.context.snapshot(), mir, &available.cancel)?;
        let module = verify_mir(metadata, &available.cancel).map_err(verification_error)?;
        Ok(LoweredModule {
            module,
            classifications: products.classifications.to_vec(),
            source_bodies: products.source_bodies.to_vec(),
        })
    }
}

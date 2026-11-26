//! Candidate-only assembly: retained bodies and bridge recipes are never regenerated.

use crate::source::lower::{
    LoweredModule, MirLoweringError, ModuleLowering, ModuleLoweringInputs, host,
    instances::MirLoweringOptions, verification_error,
};
use kagari_common::identity::metadata::DefinitionMetadata;
use kagari_contract::types::identity;
use kagari_mir::{function::MirModule, ids::InstanceId, verify::verify_mir};

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
        let mut mir = MirModule {
            native_targets: products.native_targets.to_vec(),
            interface_instances: products.interface_instances.to_vec(),
            external_types: Vec::new(),
            dependencies: inputs
                .compilation
                .dependencies(module.handle())
                .expect("checked dependencies")
                .iter()
                .map(|handle| {
                    module
                        .environment()
                        .definitions()
                        .module_identity(*handle)
                        .expect("checked dependency")
                        .clone()
                })
                .collect(),
            structures: products.structures.to_vec(),
            enumerations: products.enumerations.to_vec(),
            identity: module.source().module_identity().clone(),
            source_name: module.source().name().to_owned(),
            module_slots: self.instances.static_slots.clone(),
            abi: products.abi.clone(),
            functions,
        };
        mir.external_types =
            host::collect(inputs, &self.instances.host_types, &mir, &available.cancel)?;
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

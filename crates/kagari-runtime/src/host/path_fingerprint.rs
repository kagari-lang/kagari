//! Versioned fixed-width encoding of resolved path contracts, independent of slots.
use kagari_contract::representation::semantic_representation;

use crate::{
    error::RuntimeError,
    host::{HostPathAdapter, HostPathDescriptorId, HostPathSegment, HostRegistry},
    metadata::{TypeId, TypeRegistry},
    module::{LinkedHostBindings, admission::DefinitionAdmission},
};
use kagari_abi::representation::ValueType;
use kagari_bytecode::{instruction::BytecodeInstruction, module::BytecodeModule};
use kagari_types::{
    declaration::host::PathAccess,
    declaration::path::{HostPathContract, HostPathDeclaration, HostPathInput},
    ty::Ty,
};
use std::collections::HashMap;

/// Exact, runtime-local bindings of this importer's verified portable requirements.
/// Used only while linking; invocations retain the selected descriptor, not the map.
#[derive(Default)]
pub(crate) struct AdmittedPaths<'a> {
    entries: HashMap<&'a HostPathDeclaration, (HostPathDescriptorId, &'a HostPathAdapter)>,
}

impl AdmittedPaths<'_> {
    pub(crate) fn for_operation(
        &self,
        path: &HostPathDeclaration,
    ) -> Result<HostPathDescriptorId, RuntimeError> {
        let (id, adapter) = self.entries.get(path).ok_or_else(|| {
            RuntimeError::typed_path_validation("native operation lacks its required path binding")
        })?;
        adapter.check_access(path.access)?;
        Ok(*id)
    }
}

impl HostRegistry {
    pub(crate) fn link_module<'a>(
        &'a self,
        module: &'a BytecodeModule,
        types: &TypeRegistry,
        admission: &DefinitionAdmission,
    ) -> Result<(LinkedHostBindings, AdmittedPaths<'a>), RuntimeError> {
        let mut admitted = AdmittedPaths::default();
        let definitions = admission.source();
        for (required, contract) in module
            .path_contracts(definitions)
            .map_err(|error| RuntimeError::typed_path_validation(error.to_string()))?
        {
            let fingerprint = contract
                .fingerprint(definitions)
                .map_err(|error| RuntimeError::typed_path_validation(error.to_string()))?;
            let adopted = admission.runtime(required, &Default::default())?;
            let mut candidates = self
                .path_descriptors()
                .filter(|path| path.abi_fingerprint.0 == fingerprint && path.declaration == adopted)
                .filter_map(|path| Some((path.id, self.path_adapters.get(&path.id)?)));
            let mismatch = || {
                RuntimeError::typed_path_validation(
                    "required path has missing or ambiguous installed bindings",
                )
            };
            let binding = candidates.next().ok_or_else(mismatch)?;
            if candidates.next().is_some() {
                return Err(mismatch());
            }
            admitted.entries.insert(required, binding);
        }
        let mut host_types = HashMap::new();
        for required in &module.external_types {
            let registered = admission.runtime(required, &Default::default())?;
            let id = types
                .external_type(&registered)
                .filter(|id| self.matches_type(*id, &registered.declaration))
                .ok_or_else(|| {
                    RuntimeError::module_validation(
                        "external type lacks its exact installed metadata slot",
                    )
                })?;
            if host_types.insert(required.declaration, id).is_some() {
                return Err(RuntimeError::module_validation(
                    "external type has duplicate metadata owners",
                ));
            }
        }
        let paths = module
            .paths
            .iter()
            .map(|required| {
                let mut candidates = self
                    .path_descriptors()
                    .filter(|path| path.abi_fingerprint.0 == required.contract_fingerprint);
                let actual = candidates.next().ok_or_else(|| {
                    RuntimeError::typed_path_validation("required path contract is not registered")
                })?;
                if candidates.next().is_some() {
                    return Err(RuntimeError::typed_path_validation(
                        "required path contract has ambiguous bindings",
                    ));
                }
                let result = types.path_value_type(actual.result_type).ok_or_else(|| {
                    RuntimeError::typed_path_validation("missing path result contract")
                })?;
                if required.root_ty != ValueType::HostHandle
                    || required.result_ty != semantic_representation(&result)
                    || (!required.read_only && actual.access != PathAccess::ReadWrite)
                {
                    return Err(RuntimeError::typed_path_validation(
                        "path operand contract differs from its binding",
                    ));
                }
                Ok(actual.id)
            })
            .collect::<Result<Vec<_>, RuntimeError>>()?;
        for function in &module.functions {
            for instruction in &function.instructions {
                let (path, args) = match instruction {
                    BytecodeInstruction::ReadPath {
                        path, dynamic_args, ..
                    }
                    | BytecodeInstruction::SetPath {
                        path, dynamic_args, ..
                    }
                    | BytecodeInstruction::ModifyPath {
                        path, dynamic_args, ..
                    }
                    | BytecodeInstruction::MakePathView {
                        path, dynamic_args, ..
                    } => (path, dynamic_args),
                    _ => continue,
                };
                let descriptor = paths
                    .get(path.index())
                    .and_then(|id| self.path_descriptor(*id))
                    .ok_or_else(|| {
                        RuntimeError::typed_path_validation("missing linked path descriptor")
                    })?;
                if args.len() != descriptor.dynamic_parameters.len() {
                    return Err(RuntimeError::typed_path_validation(
                        "path dynamic argument count differs from its binding",
                    ));
                }
                for (argument, parameter) in args.iter().zip(&descriptor.dynamic_parameters) {
                    let expected = types.path_value_type(parameter.ty).ok_or_else(|| {
                        RuntimeError::typed_path_validation("missing path parameter contract")
                    })?;
                    if function.metadata.slots.get(argument.index()).copied()
                        != Some(semantic_representation(&expected))
                    {
                        return Err(RuntimeError::typed_path_validation(
                            "path dynamic argument type differs from its binding",
                        ));
                    }
                }
            }
        }
        Ok((
            LinkedHostBindings {
                types: host_types,
                paths,
                native: vec![],
            },
            admitted,
        ))
    }

    pub(super) fn path_contract(
        &self,
        declaration: &HostPathDeclaration,
        types: &TypeRegistry,
    ) -> Result<HostPathContract, RuntimeError> {
        declaration
            .contract(&types.external_declarations(), &self.definitions())
            .map_err(|error| RuntimeError::typed_path_validation(error.to_string()))
    }

    pub(super) fn checked_descriptor_contract(
        &self,
        declaration: &HostPathDeclaration,
        result_type: TypeId,
        segments: &[HostPathSegment],
        types: &TypeRegistry,
    ) -> Result<HostPathContract, RuntimeError> {
        let contract = self.path_contract(declaration, types)?;
        let portable_type = |id| {
            self.type_declarations
                .iter()
                .find_map(|(declaration, actual)| (*actual == id).then_some(Ty::Host(*declaration)))
                .or_else(|| types.path_value_type(id))
                .ok_or_else(|| {
                    RuntimeError::typed_path_validation("path type has no portable contract")
                })
        };
        if portable_type(result_type)? != contract.result
            || segments.len() != contract.segments.len()
        {
            return Err(RuntimeError::typed_path_validation(
                "path result or segment count differs from shared declaration",
            ));
        }
        for (physical, declared) in segments.iter().zip(&contract.segments) {
            let input_matches = match (physical, &declared.input) {
                (HostPathSegment::Field { owner_type, .. }, HostPathInput::Field { owner, .. }) => {
                    portable_type(*owner_type)? == *owner
                }
                (
                    HostPathSegment::Index {
                        slot,
                        collection_type,
                        index_type,
                        ..
                    },
                    HostPathInput::Index {
                        slot: declared_slot,
                        collection,
                        index,
                    },
                ) => {
                    slot.index() as u64 == *declared_slot
                        && portable_type(*collection_type)? == *collection
                        && portable_type(*index_type)? == *index
                }
                (
                    HostPathSegment::Virtual { name, .. },
                    HostPathInput::Virtual { name: declared },
                ) => name == declared,
                _ => false,
            };
            if !input_matches
                || physical.abi_fingerprint().0 != declared.member_fingerprint
                || portable_type(physical.result_type())? != declared.result
                || physical.access() != declared.access
            {
                return Err(RuntimeError::typed_path_validation(
                    "physical path segment differs from shared declaration",
                ));
            }
        }
        Ok(contract)
    }
}

//! Versioned fixed-width encoding of resolved path contracts, independent of slots.
use kagari_contract::representation::semantic_representation;

use crate::{
    error::RuntimeError,
    host::{HostPathDescriptorRegistration, HostPathSegment, HostRegistry},
    metadata::{AbiFingerprint, TypeRegistry},
    module::LinkedHostBindings,
};
use kagari_abi::representation::ValueType;
use kagari_bytecode::{instruction::BytecodeInstruction, module::BytecodeModule};
use kagari_common::identity::table::DefinitionTable;
use kagari_types::{
    declaration::host::PathAccess,
    declaration::path::{HostPathContract, HostPathDeclaration, HostPathInput},
    ty::Ty,
};
use std::collections::HashMap;

impl HostRegistry {
    pub(crate) fn link_module(
        &self,
        module: &BytecodeModule,
        types: &TypeRegistry,
        definitions: &DefinitionTable,
    ) -> Result<LinkedHostBindings, RuntimeError> {
        if definitions.id() != self.definitions().id() {
            return Err(RuntimeError::module_validation(
                "host linking requires the runtime definition context",
            ));
        }
        for (required, contract) in module
            .path_contracts(definitions)
            .map_err(|error| RuntimeError::typed_path_validation(error.to_string()))?
        {
            let fingerprint = contract
                .fingerprint(definitions)
                .map_err(|error| RuntimeError::typed_path_validation(error.to_string()))?;
            if self
                .path_descriptors()
                .filter(|path| {
                    path.abi_fingerprint.0 == fingerprint
                        && path.declaration == *required
                        && self.path_adapters.contains_key(&path.id)
                })
                .count()
                != 1
            {
                return Err(RuntimeError::typed_path_validation(
                    "required path has missing or ambiguous installed bindings",
                ));
            }
        }
        let mut host_types = HashMap::new();
        for required in &module.external_types {
            let id = types
                .external_type(required)
                .filter(|id| self.matches_type(*id, &required.declaration))
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
                    if function.metadata.registers.get(argument.index()).copied()
                        != Some(semantic_representation(&expected))
                    {
                        return Err(RuntimeError::typed_path_validation(
                            "path dynamic argument type differs from its binding",
                        ));
                    }
                }
            }
        }
        Ok(LinkedHostBindings {
            types: host_types,
            paths,
            native: vec![],
        })
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

    pub(super) fn path_fingerprint(
        &self,
        registration: &HostPathDescriptorRegistration,
        segments: &[HostPathSegment],
        types: &TypeRegistry,
    ) -> Result<AbiFingerprint, RuntimeError> {
        let root = self
            .type_declarations
            .iter()
            .find_map(|(declaration, id)| (*id == registration.root_type).then_some(*declaration))
            .ok_or_else(|| RuntimeError::typed_path_validation("missing path root contract"))?;
        let contract = self.path_contract(
            &HostPathDeclaration {
                root,
                segments: registration.segments.clone(),
                access: registration.access,
                schema_epoch: registration.schema_epoch.0,
            },
            types,
        )?;
        let portable_type = |id| {
            self.type_declarations
                .iter()
                .find_map(|(declaration, actual)| (*actual == id).then_some(Ty::Host(*declaration)))
                .or_else(|| types.path_value_type(id))
                .ok_or_else(|| {
                    RuntimeError::typed_path_validation("path type has no portable contract")
                })
        };
        if portable_type(registration.result_type)? != contract.result
            || segments.len() != contract.segments.len()
        {
            return Err(RuntimeError::typed_path_validation(
                "path result or segment count differs from shared declaration",
            ));
        }
        for (physical, declared) in segments.iter().zip(&contract.segments) {
            let input = match physical {
                HostPathSegment::Field { owner_type, .. } => HostPathInput::Field {
                    owner: portable_type(*owner_type)?,
                },
                HostPathSegment::Index {
                    slot,
                    collection_type,
                    index_type,
                    ..
                } => HostPathInput::Index {
                    slot: slot.index() as u64,
                    collection: portable_type(*collection_type)?,
                    index: portable_type(*index_type)?,
                },
                HostPathSegment::Virtual { name, .. } => {
                    HostPathInput::Virtual { name: name.clone() }
                }
            };
            if input != declared.input
                || portable_type(physical.result_type())? != declared.result
                || physical.access() != declared.access
            {
                return Err(RuntimeError::typed_path_validation(
                    "physical path segment differs from shared declaration",
                ));
            }
        }
        contract
            .fingerprint(&self.definitions())
            .map(AbiFingerprint)
            .map_err(|error| RuntimeError::typed_path_validation(error.to_string()))
    }
}

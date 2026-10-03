//! Versioned fixed-width encoding of resolved path contracts, independent of slots.

use kagari_bytecode::{instruction::BytecodeInstruction, module::BytecodeModule};
use kagari_common::host_interface::{
    path::{HostPathContract, HostPathInput, HostPathSegmentContract},
    type_declaration::PathAccess,
    value_type::HostValueType,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMapper, DefinitionRecord},
        table::{DefinitionId, DefinitionTable},
    },
};

use crate::{
    error::RuntimeError,
    host::{HostPathDescriptorRegistration, HostPathSegment, HostRegistry},
    metadata::{AbiFingerprint, TypeRegistry},
    module::LinkedHostBindings,
};
use kagari_abi::representation::ValueType;

impl HostRegistry {
    pub(crate) fn link_module(
        &self,
        module: &BytecodeModule<DefinitionId>,
        types: &TypeRegistry,
        definitions: &DefinitionTable,
    ) -> Result<LinkedHostBindings, RuntimeError> {
        let cancel = CancellationToken::default();
        let interface = module
            .host_interface
            .map_identities(&mut DefinitionMapper::new(
                &mut |id| Ok(definitions.resolve(*id)?.to_path()),
                &cancel,
            ))
            .map_err(|error| RuntimeError::module_validation(error.to_string()))?;
        let functions = self.link_interface(&interface)?;
        let host_types = module
            .host_interface
            .types
            .iter()
            .zip(&interface.types)
            .map(|(scoped, authored)| {
                self.host_type_by_declaration(&authored.id)
                    .map(|host| (scoped.id, host.type_id))
                    .ok_or_else(|| RuntimeError::module_validation("linked host type is absent"))
            })
            .collect::<Result<_, _>>()?;
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
                let result = self
                    .host_type(actual.result_type)
                    .map(|ty| HostValueType::Opaque(ty.declaration.id.clone()))
                    .or_else(|| types.host_value_type(actual.result_type))
                    .ok_or_else(|| {
                        RuntimeError::typed_path_validation("missing path result contract")
                    })?;
                if required.root_ty != ValueType::HostHandle
                    || required.result_ty != ValueType::from_host_type(&result)
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
                    let expected = self
                        .host_type(parameter.ty)
                        .map(|ty| HostValueType::Opaque(ty.declaration.id.clone()))
                        .or_else(|| types.host_value_type(parameter.ty))
                        .ok_or_else(|| {
                            RuntimeError::typed_path_validation("missing path parameter contract")
                        })?;
                    if function.metadata.registers.get(argument.index()).copied()
                        != Some(ValueType::from_host_type(&expected))
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
            functions,
            paths,
            native: vec![],
        })
    }

    pub(super) fn path_fingerprint(
        &self,
        registration: &HostPathDescriptorRegistration,
        segments: &[HostPathSegment],
        types: &TypeRegistry,
    ) -> Result<AbiFingerprint, RuntimeError> {
        let portable_type = |id| {
            self.host_type(id)
                .map(|info| HostValueType::Opaque(info.declaration.id.clone()))
                .or_else(|| types.host_value_type(id))
                .ok_or_else(|| {
                    RuntimeError::typed_path_validation("path type has no portable host contract")
                })
        };
        let root = self
            .host_type(registration.root_type)
            .ok_or_else(|| RuntimeError::typed_path_validation("missing path root contract"))?;
        let segments = segments
            .iter()
            .map(|segment| {
                let input = match segment {
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
                Ok(HostPathSegmentContract {
                    input,
                    result: portable_type(segment.result_type())?,
                    access: segment.access(),
                    member_fingerprint: segment.abi_fingerprint().0,
                })
            })
            .collect::<Result<Vec<_>, RuntimeError>>()?;
        HostPathContract {
            root_fingerprint: root.abi_fingerprint.0,
            result: portable_type(registration.result_type)?,
            schema_epoch: registration.schema_epoch.0,
            access: registration.access,

            segments,
        }
        .fingerprint()
        .map(AbiFingerprint)
        .map_err(|error| RuntimeError::typed_path_validation(error.to_string()))
    }
}

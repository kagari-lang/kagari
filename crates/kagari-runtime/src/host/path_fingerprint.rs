//! Versioned fixed-width encoding of resolved path contracts, independent of slots.
use super::*;
use crate::metadata::TypeRegistry;

impl HostRegistry {
    pub(crate) fn link_module(
        &self,
        module: &kagari_ir::bytecode::BytecodeModule,
        types: &TypeRegistry,
    ) -> Result<crate::module::LinkedHostBindings, RuntimeError> {
        let functions = self.link_interface(&module.host_interface)?;
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
                if required.root_ty != kagari_ir::module::ValueType::HostHandle
                    || required.result_ty != kagari_ir::module::ValueType::from_host_type(&result)
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
                use kagari_ir::bytecode::BytecodeInstruction as I;
                let (path, args) = match instruction {
                    I::ReadPath {
                        path, dynamic_args, ..
                    }
                    | I::SetPath {
                        path, dynamic_args, ..
                    }
                    | I::ModifyPath {
                        path, dynamic_args, ..
                    }
                    | I::MakePathView {
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
                        != Some(kagari_ir::module::ValueType::from_host_type(&expected))
                    {
                        return Err(RuntimeError::typed_path_validation(
                            "path dynamic argument type differs from its binding",
                        ));
                    }
                }
            }
        }
        Ok(crate::module::LinkedHostBindings { functions, paths })
    }

    pub(super) fn path_fingerprint(
        &self,
        registration: &HostPathDescriptorRegistration,
        segments: &[HostPathSegment],
        types: &TypeRegistry,
    ) -> Result<AbiFingerprint, RuntimeError> {
        use kagari_common::host_interface::{
            HostPathContract, HostPathInput, HostPathSegmentContract,
        };
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
            capabilities: registration.capability_requirements,
            segments,
        }
        .fingerprint()
        .map(AbiFingerprint)
        .map_err(|error| RuntimeError::typed_path_validation(error.to_string()))
    }
}

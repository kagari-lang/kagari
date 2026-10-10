//! Host preparation selects installed executable evidence, never source specializations.
use crate::{
    Runtime,
    error::RuntimeError,
    execution_metadata::MetadataRoot,
    frame::types::{
        arguments::{ScopedSignature, TypeArgument},
        compatibility::TypeView,
    },
    module::{LoadedModule, ModuleKey},
    native::{
        binding::NativeResult,
        context::{CallableOwner, LinkedCallable},
    },
};
use kagari_bytecode::{
    instruction::{BytecodeInstruction, CallTarget, NativeImportId},
    module::CallableTarget,
};
use kagari_common::identity::{DefinitionPath, table::DefinitionId};
use kagari_contract::{callable::shared::SharedCall, ids::FunctionRef};
use kagari_types::ty::Ty;
use std::{slice, sync::Arc};

#[derive(Debug, PartialEq, Eq)]
pub(super) struct EvidenceKey {
    owner: ModuleKey,
    target: CallableTarget,
    shared: Option<(ModuleKey, FunctionRef, usize)>,
}

pub(super) struct EntryEvidence {
    pub(super) owner: LoadedModule,
    pub(super) key: EvidenceKey,
    pub(super) signature: Arc<ScopedSignature>,
    params: Vec<Ty<DefinitionId>>,
    result: Ty<DefinitionId>,
    shared: Option<(LoadedModule, SharedCall<DefinitionId>)>,
}

impl EntryEvidence {
    pub(super) fn find(
        runtime: &Runtime,
        root: &LoadedModule,
        declaration: &DefinitionPath,
        arguments: &[TypeArgument],
    ) -> NativeResult<Self> {
        let same_declaration = |owner: &LoadedModule, id| {
            owner
                .definitions()
                .resolve(id)
                .is_ok_and(|id| id.to_path() == *declaration)
        };
        let same_arguments = |owner: &LoadedModule, types: &[Ty<DefinitionId>]| {
            types.len() == arguments.len()
                && types.iter().zip(arguments).all(|(ty, argument)| {
                    ty.is_concrete()
                        && argument.ty() == ty
                        && argument
                            .view(root)
                            .compatible(TypeView::new(ty, owner, None))
                })
        };
        for owner in root.members() {
            for function in &owner.bytecode.functions {
                if function.metadata.semantic.generic.is_some()
                    || !function.identity.as_ref().is_some_and(|identity| {
                        same_declaration(&owner, identity.declaration)
                            && same_arguments(&owner, &identity.arguments)
                    })
                {
                    continue;
                }
                let params = (0..usize::from(function.parameter_count))
                    .map(|index| {
                        function
                            .metadata
                            .semantic
                            .params
                            .get(&index)
                            .cloned()
                            .ok_or_else(missing)
                    })
                    .collect::<NativeResult<Vec<_>>>()?;
                return Self::closed(
                    runtime,
                    &owner,
                    CallableTarget::Script(function.id),
                    params,
                    function
                        .metadata
                        .semantic
                        .result
                        .clone()
                        .ok_or_else(missing)?,
                );
            }
            for (index, import) in owner.bytecode.native_imports.iter().enumerate() {
                if import.generic.is_none()
                    && same_declaration(&owner, import.instance.declaration)
                    && same_arguments(&owner, &import.instance.arguments)
                {
                    return Self::closed(
                        runtime,
                        &owner,
                        CallableTarget::Native(NativeImportId::new(index)),
                        import.signature.params.clone(),
                        import.signature.result.clone(),
                    );
                }
            }
        }
        // A shared body alone proves no particular application. A closed call
        // witness supplies its types, required operations and exact target.
        for carrier in root.members() {
            for function in &carrier.bytecode.functions {
                if function.metadata.semantic.generic.is_some() {
                    continue;
                }
                for (offset, instruction) in function.instructions.iter().enumerate() {
                    let BytecodeInstruction::Call {
                        callee:
                            CallTarget::Shared {
                                module,
                                target,
                                contract,
                            },
                        ..
                    } = instruction
                    else {
                        continue;
                    };
                    if !same_declaration(&carrier, contract.instance.declaration)
                        || !same_arguments(&carrier, &contract.arguments)
                    {
                        continue;
                    }
                    let owner = carrier.member(*module).ok_or_else(missing)?;
                    let signature = Arc::new(ScopedSignature {
                        params: runtime
                            .resolve_type_arguments(&carrier, &contract.signature.params)?,
                        result: runtime
                            .resolve_type_arguments(
                                &carrier,
                                slice::from_ref(&contract.signature.result),
                            )?
                            .pop()
                            .ok_or_else(missing)?,
                    });
                    return Ok(Self {
                        key: EvidenceKey {
                            owner: owner.key(),
                            target: *target,
                            shared: Some((carrier.key(), function.id, offset)),
                        },
                        owner,
                        signature,
                        params: contract.signature.params.clone(),
                        result: contract.signature.result.clone(),
                        shared: Some((carrier.clone(), contract.as_ref().clone())),
                    });
                }
            }
        }
        Err(missing())
    }

    fn closed(
        runtime: &Runtime,
        owner: &LoadedModule,
        target: CallableTarget,
        params: Vec<Ty<DefinitionId>>,
        result: Ty<DefinitionId>,
    ) -> NativeResult<Self> {
        let signature = Arc::new(ScopedSignature {
            params: runtime.resolve_type_arguments(owner, &params)?,
            result: runtime
                .resolve_type_arguments(owner, slice::from_ref(&result))?
                .pop()
                .ok_or_else(missing)?,
        });
        Ok(Self {
            owner: owner.clone(),
            key: EvidenceKey {
                owner: owner.key(),
                target,
                shared: None,
            },
            signature,
            params,
            result,
            shared: None,
        })
    }

    pub(super) fn prepare(&self, runtime: &Runtime) -> NativeResult<LinkedCallable> {
        let environment = self
            .shared
            .as_ref()
            .map(|(carrier, contract)| {
                runtime.prepare_shared_environment(
                    carrier,
                    None,
                    &self.owner,
                    self.key.target,
                    contract,
                )
            })
            .transpose()?;
        let owner = if let Some(environment) = &environment {
            let roots = runtime.root_metadata(vec![
                MetadataRoot::Program(self.owner.clone()),
                MetadataRoot::Environment(environment.id),
            ])?;
            CallableOwner::Pinned(self.owner.clone(), roots)
        } else {
            // PreparedFunction already owns a program lease. Only an executable
            // environment needs an additional metadata root.
            CallableOwner::Program(self.owner.slot())
        };
        Ok(LinkedCallable {
            environment,
            scoped_signature: Some(self.signature.clone()),
            owner,
            target: self.key.target,
            params: self.params.clone().into_boxed_slice(),
            result: self.result.clone(),
            primitive: None,
        })
    }
}

fn missing() -> RuntimeError {
    RuntimeError::module_validation("function application has no checked executable evidence")
}

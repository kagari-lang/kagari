//! Host preparation selects installed executable evidence, never source specializations.
use crate::{
    Runtime,
    error::RuntimeError,
    execution_metadata::MetadataRoot,
    frame::types::{
        arguments::{ScopedSignature, TypeArgument},
        compatibility::TypeView,
    },
    module::{
        ExecutableModule, ModuleKey,
        dispatch::{CallRoute, identity::PublicationDefinitions},
    },
    native::{
        binding::NativeResult,
        context::{CallableOwner, LinkedCallable},
        function_handle::PreparedFunction,
    },
};
use kagari_bytecode::{
    instruction::{BytecodeInstruction, CallTarget, NativeImportId},
    module::CallableTarget,
};
use kagari_common::identity::table::DefinitionId;
use kagari_contract::{callable::shared::SharedCall, ids::FunctionRef};
use kagari_types::{callable::Signature, ty::Ty};
use std::{slice, sync::Arc};

#[derive(Debug, PartialEq, Eq)]
pub(super) struct EvidenceKey {
    owner: ModuleKey,
    target: CallableTarget,
    shared: Option<(ModuleKey, FunctionRef, usize)>,
}

/// Checked binding metadata, with no executable lease or environment root.
#[derive(Debug)]
pub(super) struct EntryBinding {
    route: CallRoute,
    signature: Signature,
    shared: Option<(Arc<PublicationDefinitions>, SharedCall)>,
}

impl EntryBinding {
    pub(super) fn route(&self) -> &CallRoute {
        &self.route
    }

    pub(super) fn prepare(&self, runtime: &Runtime) -> NativeResult<PreparedFunction> {
        let selected = runtime.select_call_route(&self.route)?;
        let owner = selected.owner();
        let shared = self
            .shared
            .as_ref()
            .map(|(scope, contract)| {
                selected
                    .mapping_from(scope.clone())?
                    .remap(contract, &Default::default())
                    .map(|metadata| metadata.into_records())
            })
            .transpose()?;
        let signature = match &shared {
            Some(contract) => contract.signature.clone(),
            None => selected
                .mapping()
                .remap(&self.signature, &Default::default())?
                .into_records(),
        };
        let scoped_signature = Arc::new(ScopedSignature {
            params: runtime.resolve_type_arguments(owner, &signature.params)?,
            result: runtime
                .resolve_type_arguments(owner, slice::from_ref(&signature.result))?
                .pop()
                .ok_or_else(missing)?,
        });
        let environment = shared
            .as_ref()
            .map(|contract| {
                runtime.prepare_selected_shared_environment(owner, None, &selected, contract)
            })
            .transpose()?;
        let mut roots = vec![MetadataRoot::Program(owner.clone())];
        roots.extend(
            environment
                .iter()
                .map(|environment| MetadataRoot::Environment(environment.id)),
        );
        let target = LinkedCallable {
            environment,
            scoped_signature: Some(scoped_signature),
            owner: CallableOwner::Pinned(owner.clone(), runtime.root_metadata(roots)?),
            target: selected.target(),
            params: signature.params.clone().into_boxed_slice(),
            result: signature.result.clone(),
            primitive: None,
        };
        PreparedFunction::selected(runtime, owner, target)
    }
}

pub(super) struct EntryEvidence {
    pub(super) owner: ExecutableModule,
    pub(super) key: EvidenceKey,
    pub(super) signature: Arc<ScopedSignature>,
    shared: Option<(ExecutableModule, SharedCall)>,
}

impl EntryEvidence {
    pub(super) fn script(
        runtime: &Runtime,
        owner: &ExecutableModule,
        target: FunctionRef,
    ) -> NativeResult<Self> {
        let function = owner
            .bytecode
            .functions
            .get(target.index())
            .ok_or_else(missing)?;
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
        Self::closed(
            runtime,
            owner,
            CallableTarget::Script(target),
            params,
            function
                .metadata
                .semantic
                .result
                .clone()
                .ok_or_else(missing)?,
        )
    }

    pub(super) fn find(
        runtime: &Runtime,
        root: &ExecutableModule,
        declaration: DefinitionId,
        arguments: &[TypeArgument],
    ) -> NativeResult<Self> {
        root.definitions()
            .resolve(declaration)
            .map_err(|_| missing())?;
        let same_arguments = |owner: &ExecutableModule, types: &[Ty]| {
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
                        identity.declaration == declaration
                            && same_arguments(&owner, &identity.arguments)
                    })
                {
                    continue;
                }
                return Self::script(runtime, &owner, function.id);
            }
            for (index, import) in owner.bytecode.native_imports.iter().enumerate() {
                if import.generic.is_none()
                    && import.instance.declaration == declaration
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
                    if contract.instance.declaration != declaration
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

                        shared: Some((carrier.clone(), contract.as_ref().clone())),
                    });
                }
            }
        }
        Err(missing())
    }

    fn closed(
        runtime: &Runtime,
        owner: &ExecutableModule,
        target: CallableTarget,
        params: Vec<Ty>,
        result: Ty,
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
            shared: None,
        })
    }

    pub(super) fn bind(&self) -> NativeResult<EntryBinding> {
        Ok(EntryBinding {
            route: self.owner.bind_call_route(self.key.target)?,
            signature: Signature {
                params: self
                    .signature
                    .params
                    .iter()
                    .map(|argument| argument.ty().clone())
                    .collect(),
                result: self.signature.result.ty().clone(),
            },
            shared: self
                .shared
                .as_ref()
                .map(|(carrier, contract)| (carrier.publication_definitions(), contract.clone())),
        })
    }
}

fn missing() -> RuntimeError {
    RuntimeError::module_validation("function application has no checked executable evidence")
}

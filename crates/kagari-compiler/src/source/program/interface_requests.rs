//! Foreign table-slot obligations independent of source call reachability.
use crate::source::program::{SourceProgramError, update_requests::RequestQueue};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{ModuleIdentity, table::DefinitionTable},
};
use kagari_contract::types::ConcreteFunctionIdentity;
use kagari_hir::{
    AnalyzedModule, resolver::resolved::ResolvedName, typeck::FunctionImplementation,
};
use kagari_mir::program::{ProgramError, ProgramErrorKind};
use kagari_types::ty::GenericParam;

pub(super) struct ForeignInterfaces<'a> {
    pub modules: &'a [AnalyzedModule],
    pub root: &'a ModuleIdentity,
    pub definitions: &'a DefinitionTable,
    pub cancel: &'a CancellationToken,
}

impl ForeignInterfaces<'_> {
    pub fn request(
        &self,
        caller: &ModuleIdentity,
        instance: &ConcreteFunctionIdentity,
        requests: &mut RequestQueue,
    ) -> Result<(), SourceProgramError> {
        let definitions = self.definitions;
        let root = self.root;
        let implementation = &instance.declaration;
        let arguments = &instance.arguments;
        if arguments.is_empty()
            || definitions
                .resolve(*implementation)
                .expect("checked implementation")
                .module()
                == caller
        {
            return Ok(());
        }
        self.cancel.check().map_err(|_| ProgramError {
            module: Box::new(root.clone()),
            kind: ProgramErrorKind::Cancelled,
        })?;
        let owner = self
            .modules
            .iter()
            .find(|module| {
                module.lowered.source.module_identity()
                    == definitions
                        .resolve(*implementation)
                        .expect("checked implementation")
                        .module()
            })
            .ok_or_else(|| ProgramError {
                module: Box::new(root.clone()),
                kind: ProgramErrorKind::InterfaceContract(
                    definitions
                        .resolve(*implementation)
                        .expect("checked implementation")
                        .to_path(),
                ),
            })?;
        let signature = owner
            .aggregates
            .implementation_signature(implementation)
            .ok_or_else(|| ProgramError {
                module: Box::new(root.clone()),
                kind: ProgramErrorKind::InterfaceContract(
                    definitions
                        .resolve(*implementation)
                        .expect("checked implementation")
                        .to_path(),
                ),
            })?;
        for method in owner
            .aggregates
            .implementation_methods(definitions, signature)
        {
            if let Some(ResolvedName::Function(function)) =
                owner.declarations.definition_target(&method)
                && owner.typed.functions.iter().any(|typed| {
                    typed.id == function
                        && matches!(typed.implementation, FunctionImplementation::Native(_))
                })
                && !owner
                    .lowered
                    .registered_native_declarations()
                    .iter()
                    .any(|declaration| declaration.declaration == method)
            {
                // Legacy engine/host adapters retain their existing lowering.
                continue;
            }
            if owner
                .aggregates
                .trait_(&signature.trait_type.declaration)
                .is_some_and(|contract| {
                    contract.methods.iter().any(|required| {
                        definitions
                            .resolve(required.id)
                            .expect("checked method")
                            .last()
                            == definitions.resolve(method).expect("checked method").last()
                            && required.generic_params.len() > contract.generic_params.len()
                            && required
                                .params
                                .first()
                                .is_none_or(|parameter| parameter.name != "self")
                    })
                })
            {
                // Static generic methods do not occupy an interface receiver slot.
                continue;
            }
            // A table can expose methods whose additional bounds are proved
            // only at a call site. Keep their receiver binders in a shared
            // native entry rather than demanding an uncallable specialization.
            let conditional = owner
                .declarations
                .definition_target(&method)
                .and_then(|target| match target {
                    ResolvedName::Function(id) => owner.typed.functions.iter().find(|f| f.id == id),
                    _ => None,
                })
                .is_some_and(|function| {
                    matches!(function.implementation, FunctionImplementation::Native(_))
                        && function.bounds.iter().any(|(ty, constraints)| {
                            constraints.iter().any(|constraint| {
                                !signature
                                    .bounds
                                    .get(ty)
                                    .is_some_and(|assumed| assumed.contains(constraint))
                            })
                        })
                });
            let instance = ConcreteFunctionIdentity {
                declaration: method,
                arguments: if conditional {
                    arguments
                        .iter()
                        .enumerate()
                        .map(|(position, _)| {
                            GenericParam {
                                owner: method,
                                position,
                            }
                            .as_type()
                        })
                        .collect()
                } else {
                    arguments.clone()
                },
            };
            requests.function(instance, definitions).map_err(|error| {
                SourceProgramError::Lowering {
                    module: Box::new(root.clone()),
                    error,
                }
            })?;
        }
        Ok(())
    }
}

//! Forward trait authoring and implementation-local facts share the ordinary
//! declaration lowerer and catalog proofs; no completed trait handles are invented.
use crate::native::{
    assembly::{
        authoring::PendingBinding,
        conflict,
        draft::{ModuleDraft, TraitImplementation},
        references::{DraftCallableRequirement, DraftMember, DraftReference, ReferenceKind},
    },
    binding::NativeResult,
    catalog::DeclarationCatalog,
    declarations::{CallableRequirement, FunctionBuilder, SelectedCall},
    types::Type,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind, map::DefinitionContext, metadata::DefinitionMetadata, table::DefinitionId,
    },
};
use kagari_types::{
    declaration::{TraitDef, module::ModuleDecl, requirement::NativeCallableRequirement},
    ty::{
        GenericBound, Ty,
        substitution::{TypeSubstitution, resolve_associated_outputs},
    },
};

#[cfg(test)]
mod tests;

impl ModuleDraft<'_> {
    /// Reference a trait method in this batch or its adopted providers. This does
    /// not permit defining or mutating members owned by another module.
    pub fn reference_method(
        &mut self,
        owner: &DraftReference,
        name: &str,
        arity: usize,
    ) -> NativeResult<DraftMember> {
        self.reference_member(owner, DefinitionKind::Method, name, arity)
    }

    /// Reference an associated type before the owning recipe has run.
    pub fn reference_associated_type(
        &mut self,
        owner: &DraftReference,
        name: &str,
        arity: usize,
    ) -> NativeResult<DraftMember> {
        self.reference_member(owner, DefinitionKind::AssociatedType, name, arity)
    }

    fn reference_member(
        &mut self,
        owner: &DraftReference,
        kind: DefinitionKind,
        name: &str,
        arity: usize,
    ) -> NativeResult<DraftMember> {
        if owner.definitions.id() != self.definitions().id() {
            return Err(conflict("foreign assembly trait reference"));
        }
        if owner.kind != ReferenceKind::Trait {
            return Err(conflict("forward member requires a trait owner"));
        }
        let id = self
            .context
            .intern_child(owner.id, kind, name, 0)
            .map_err(conflict)?;
        let mut owner = owner.clone();
        owner.definitions = self.definitions();
        let member = DraftMember {
            owner,
            id,
            kind,
            name: name.into(),
            arity,
        };
        self.state.members.push(member.clone());
        Ok(member)
    }
}

impl FunctionBuilder<'_> {
    /// Add a forward same-assembly requirement through the ordinary selected-call
    /// slot allocator. Foreign draft handles reject rather than being imported.
    pub fn requires_draft(
        &mut self,
        requirement: DraftCallableRequirement,
    ) -> NativeResult<SelectedCall> {
        let requirement = requirement.lower(self.context)?;
        self.requires(CallableRequirement {
            requirement: DefinitionMetadata::checked(
                self.context.snapshot(),
                requirement,
                &CancellationToken::default(),
            )
            .map_err(conflict)?,
        })
    }

    /// Add a method-local bound using an applied forward trait type. Existence,
    /// binder scope and applicability are checked by the ordinary sealing proofs.
    pub fn bound_draft(&mut self, ty: Type, interface: Type) -> NativeResult<()> {
        let ty = ty.adopt(self.context)?;
        let Ty::Trait(interface) = interface.adopt(self.context)? else {
            return Err(conflict("method bound requires a trait application"));
        };
        self.signature.bounds.push(GenericBound {
            ty,
            constraints: vec![interface],
        });
        Ok(())
    }
}

/// One explicit trait override, including facts specific to its native body.
/// Signatures still come exclusively from the trait contract. Facts may mention
/// trait Self/parameters, trait method parameters, and actual impl parameters;
/// sealing substitutes the first three into the real implementation binders.
pub struct TraitOverride {
    pub(crate) member: DraftMember,
    binding: PendingBinding,
    requirements: Vec<DraftCallableRequirement>,
    concrete_result: Option<Type>,
    documentation: Option<String>,
}

impl TraitOverride {
    pub fn new(member: DraftMember, binding: PendingBinding) -> Self {
        Self {
            member,
            binding,
            requirements: vec![],
            concrete_result: None,
            documentation: None,
        }
    }

    /// Append in selected-call slot order; duplicate obligations retain distinct slots.
    pub fn requires(mut self, requirement: DraftCallableRequirement) -> Self {
        self.requirements.push(requirement);
        self
    }

    /// Attach implementation-specific Markdown without changing its trait signature.
    pub fn documentation(mut self, text: impl Into<String>) -> Self {
        self.documentation = Some(text.into());
        self
    }

    pub fn produces(mut self, ty: Type) -> Self {
        self.concrete_result = Some(ty);
        self
    }

    pub(crate) fn lower(
        self,
        context: &DefinitionContext,
    ) -> NativeResult<(OverrideFacts, PendingBinding)> {
        let requirements = self
            .requirements
            .iter()
            .map(|requirement| requirement.lower(context))
            .collect::<NativeResult<_>>()?;
        let concrete_result = self
            .concrete_result
            .map(|ty| ty.adopt(context))
            .transpose()?;
        Ok((
            OverrideFacts {
                member: self.member,
                requirements,
                concrete_result,
                documentation: self.documentation,
            },
            self.binding,
        ))
    }
}

/// A complete implementation with identity-addressed, fact-bearing overrides.
/// Raw binding tuples remain the low-level path; both feed the same intent list.
pub struct TraitImplementationDefinition {
    pub(crate) implementation: TraitImplementation,
    pub(crate) overrides: Vec<TraitOverride>,
}

impl TraitImplementation {
    pub fn with_overrides(
        self,
        overrides: impl IntoIterator<Item = TraitOverride>,
    ) -> TraitImplementationDefinition {
        TraitImplementationDefinition {
            implementation: self,
            overrides: overrides.into_iter().collect(),
        }
    }
}

impl From<TraitImplementation> for TraitImplementationDefinition {
    fn from(implementation: TraitImplementation) -> Self {
        implementation.with_overrides([])
    }
}

pub(crate) struct OverrideIntent {
    pub(crate) name: String,
    pub(crate) id: DefinitionId,
    pub(crate) facts: Option<OverrideFacts>,
}

pub(crate) struct OverrideFacts {
    member: DraftMember,
    requirements: Vec<NativeCallableRequirement>,
    concrete_result: Option<Ty>,
    documentation: Option<String>,
}

/// Run only after implement_trait has established the actual signatures/binders,
/// and before building the final catalog. This transforms facts, not signatures.
pub(crate) fn lower_override_facts(
    contracts: &DeclarationCatalog,
    module: &mut ModuleDecl,
    contract: &TraitDef,
    index: usize,
    intents: Vec<OverrideIntent>,
) -> NativeResult<()> {
    let implementation = &module.implementations[index];
    let interface = implementation.trait_type.as_ref().expect("trait intent");
    let cancel = CancellationToken::default();
    for intent in intents {
        let Some(facts) = intent.facts else { continue };
        facts.member.validate(contracts)?;
        let source = contract
            .methods
            .iter()
            .find(|method| method.name == intent.name)
            .ok_or_else(|| conflict("unknown override method"))?;
        let target = implementation
            .methods
            .iter()
            .find(|method| method.name == intent.name)
            .ok_or_else(|| conflict("missing lowered override method"))?;
        let own_types = target
            .generic_params
            .iter()
            .filter(|parameter| parameter.owner == intent.id)
            .map(|parameter| parameter.as_type())
            .collect::<Vec<_>>();
        if own_types.len() != source.generic_params.len() {
            return Err(conflict("override method binder disagreement"));
        }
        let mut substitution =
            TypeSubstitution::for_owner(&interface.declaration, &interface.arguments);
        substitution.bind_receiver(&interface.declaration, &implementation.for_type);
        for (parameter, ty) in source.generic_params.iter().zip(&own_types) {
            substitution.bind(&parameter.owner, parameter.position, ty);
        }
        let resolve = |ty: &Ty| -> NativeResult<Ty> {
            let ty = substitution
                .apply(ty, &cancel)
                .map_err(|error| conflict(format!("{error:?}")))?;
            resolve_associated_outputs(&ty, interface, &implementation.for_type, &cancel)
                .map_err(|error| conflict(format!("{error:?}")))
        };
        let requirements = facts
            .requirements
            .into_iter()
            .map(|requirement| {
                let Ty::Trait(interface) = resolve(&Ty::Trait(requirement.interface))? else {
                    unreachable!("trait substitution")
                };
                Ok(NativeCallableRequirement {
                    receiver: resolve(&requirement.receiver)?,
                    interface,
                    member: requirement.member,
                    arguments: requirement
                        .arguments
                        .iter()
                        .map(&resolve)
                        .collect::<NativeResult<_>>()?,
                })
            })
            .collect::<NativeResult<Vec<_>>>()?;
        if !requirements.is_empty() {
            module.callable_requirements.insert(intent.id, requirements);
        }
        if let Some(result) = facts.concrete_result {
            module.concrete_results.insert(intent.id, resolve(&result)?);
        }
        if let Some(documentation) = facts.documentation {
            module.documentation.insert(intent.id, documentation);
        }
    }
    Ok(())
}

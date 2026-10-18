//! Complete trait-default submissions and deferred semantic lowering.
use crate::native::{
    assembly::{
        authoring::{MethodDefinition, PendingBinding},
        conflict,
        draft::{DraftState, ModuleDraft},
        references::{DraftReference, ReferenceKind},
    },
    binding::NativeResult,
    builder::default_lowering::{DefaultTemplateSpec, lower_default, trait_default_assumptions},
    declarations::{FunctionBuilder, normalize_bounds},
    types::{FunctionRef, Type, same_definition},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionKind, map::DefinitionContext, table::DefinitionId},
};
use kagari_types::{
    callable::CallableImplementation,
    declaration::requirement::NativeCallableRequirement,
    ty::{GenericBound, GenericParam, Ty},
};
use std::{collections::BTreeMap, mem};

/// A complete default method with optional explicit template application facts.
///
/// Ordinary defaults accept a `MethodDefinition` directly. For associated outputs,
/// wrap it in `DefaultDefinition::new` and supply authored arguments in template
/// order, for example `Self, T, Self::Iter`. These are not lowered template types.
/// Bounds and requirements use the same authored scope; sealing checks their proof
/// against the complete trait graph before any native factory can run.
pub struct DefaultDefinition {
    definition: MethodDefinition,
    arguments: Option<Vec<Type>>,
    bounds: Vec<GenericBound>,
    assumptions: Vec<GenericBound>,
    requirements: Vec<NativeCallableRequirement>,
}

impl DefaultDefinition {
    pub fn new(definition: MethodDefinition) -> Self {
        Self {
            definition,
            arguments: None,
            bounds: vec![],
            assumptions: vec![],
            requirements: vec![],
        }
    }

    /// Replace the ordinary Self/trait/method ordering with explicit lifted inputs.
    /// Scoped `Type` inputs are adopted into this assembly before sealing.
    pub fn arguments(mut self, arguments: impl IntoIterator<Item = Type>) -> Self {
        self.arguments = Some(arguments.into_iter().collect());
        self
    }

    /// Add method bounds involving forward trait references in this draft's scope.
    /// Unlike assumptions, these are part of the public method contract.
    pub fn bounds(mut self, bounds: impl IntoIterator<Item = GenericBound>) -> Self {
        self.bounds.extend(bounds);
        self
    }

    /// Add authored bounds to the ordinary trait and method assumptions.
    /// Raw semantic IDs must belong to this draft's definition table.
    pub fn assumptions(mut self, assumptions: impl IntoIterator<Item = GenericBound>) -> Self {
        self.assumptions.extend(assumptions);
        self
    }

    /// Append explicit callable facts after requirements added through configure.
    /// Their order is retained. Raw semantic IDs must belong to this draft's table.
    pub fn requirements(
        mut self,
        requirements: impl IntoIterator<Item = NativeCallableRequirement>,
    ) -> Self {
        self.requirements.extend(requirements);
        self
    }
}

impl From<MethodDefinition> for DefaultDefinition {
    fn from(definition: MethodDefinition) -> Self {
        Self::new(definition)
    }
}

pub(crate) struct DefaultIntent {
    owner: DefinitionId,
    method: DefinitionId,
    arguments: Option<Vec<Ty>>,
    assumptions: Vec<GenericBound>,
    requirements: Vec<NativeCallableRequirement>,
    concrete_result: Option<Ty>,
    binding: PendingBinding,
}

impl ModuleDraft<'_> {
    /// Submit one complete default under an already authored local trait.
    ///
    /// Configuration runs once using the authored method binder. Failure publishes
    /// no method or intent. Hidden arguments and implementation preparation remain
    /// independent: sealing lowers the former; only `into_modules` runs the latter.
    pub fn define_trait_default(
        &mut self,
        owner: &DraftReference,
        definition: impl Into<DefaultDefinition>,
    ) -> NativeResult<FunctionRef> {
        self.check_local(owner.id)?;
        if owner.kind != ReferenceKind::Trait {
            return Err(conflict("default owner is not a trait"));
        }
        let definitions = self.definitions();
        let name = definitions
            .resolve(owner.id)
            .map_err(conflict)?
            .last()
            .ok_or_else(|| conflict("default owner is not a declaration"))?
            .name;
        let index = self
            .state
            .declaration
            .traits
            .iter()
            .position(|record| record.name == name && record.generic_params.len() == owner.arity)
            .ok_or_else(|| conflict("default owner is not an authored local trait"))?;
        let DefaultDefinition {
            definition,
            arguments,
            bounds,
            assumptions,
            requirements: extra_requirements,
        } = definition.into();
        let MethodDefinition {
            mut declaration,
            concrete_result,
            configure,
            binding,
        } = definition;
        if self.state.declaration.traits[index]
            .methods
            .iter()
            .any(|method| method.name == declaration.signature.name)
        {
            return Err(conflict("duplicate trait method declaration"));
        }
        let id = self.member(
            owner.id,
            DefinitionKind::Method,
            &declaration.signature.name,
        )?;
        let documentation = declaration.signature.documentation.take();
        let receiver = Type::from_semantic(Ty::SelfType(owner.id), &definitions)?;
        let mut signature = declaration.lower(receiver, self.context)?;
        let mut requirements = vec![];
        let mut concrete_results = BTreeMap::new();
        let mut names = vec![];
        let mut function = FunctionBuilder {
            id,
            context: self.context,
            signature: &mut signature,
            concrete_results: &mut concrete_results,
            requirements: &mut requirements,
            names: &mut names,
        };
        if let Some(ty) = concrete_result {
            function.produces(ty)?;
        }
        if let Some(configure) = configure {
            configure(&mut function)?;
        }
        signature.bounds.extend(bounds);
        normalize_bounds(&mut signature, &self.definitions())?;
        let arguments = arguments
            .map(|arguments| {
                arguments
                    .iter()
                    .map(|ty| ty.adopt(self.context))
                    .collect::<NativeResult<Vec<_>>>()
            })
            .transpose()?;
        for bound in &assumptions {
            self.check_scope(bound)?;
        }
        for requirement in &extra_requirements {
            self.check_scope(requirement)?;
        }
        self.check_scope(&signature)?;
        requirements.extend(extra_requirements);
        self.state.declaration.traits[index].methods.push(signature);
        if let Some(text) = documentation {
            self.state.declaration.documentation.insert(id, text);
        }
        self.state.default_intents.push(DefaultIntent {
            owner: owner.id,
            method: id,
            arguments,
            assumptions,
            requirements,
            concrete_result: concrete_results.remove(&id),
            binding,
        });
        Ok(FunctionRef {
            id,
            definitions: self.definitions(),
        })
    }
}

/// Lower only after all recipes have completed, then let seal rebuild its catalog.
pub(crate) fn lower_defaults(
    context: &DefinitionContext,
    draft: &mut DraftState,
) -> NativeResult<()> {
    for intent in mem::take(&mut draft.default_intents) {
        let definitions = context.snapshot();
        let owner = definitions
            .resolve(intent.owner)
            .map_err(conflict)?
            .last()
            .ok_or_else(|| conflict("default owner has no name"))?
            .name;
        let method_name = definitions
            .resolve(intent.method)
            .map_err(conflict)?
            .last()
            .ok_or_else(|| conflict("default method has no name"))?
            .name;
        let contract = draft
            .declaration
            .traits
            .iter_mut()
            .find(|record| record.name == owner)
            .ok_or_else(|| conflict("missing default trait"))?;
        let index = contract
            .methods
            .iter()
            .position(|method| method.name == method_name)
            .ok_or_else(|| conflict("missing authored default method"))?;
        let authored = contract.methods[index].clone();
        let arguments = intent.arguments.unwrap_or_else(|| {
            [Ty::SelfType(intent.owner)]
                .into_iter()
                .chain(
                    contract
                        .generic_params
                        .iter()
                        .chain(&authored.generic_params)
                        .map(GenericParam::as_type),
                )
                .collect()
        });
        let mut assumptions = trait_default_assumptions(intent.owner, contract);
        assumptions.extend(intent.assumptions);
        let name = format!("__default_{}_{}_{}", owner.len(), owner, method_name);
        if draft
            .declaration
            .functions
            .iter()
            .any(|function| function.name == name)
        {
            return Err(conflict("duplicate native default template"));
        }
        let root = context
            .intern_root(&draft.declaration.identity)
            .map_err(conflict)?;
        let template = context
            .intern_child(root, DefinitionKind::Function, &name, 0)
            .map_err(conflict)?;
        let lowered = lower_default(
            DefaultTemplateSpec {
                method: &authored,
                template,
                arguments: &arguments,
                assumptions: &assumptions,
                requirements: &intent.requirements,
                concrete_result: intent.concrete_result.as_ref(),
            },
            &context.snapshot(),
            &CancellationToken::default(),
        )
        .map_err(conflict)?;
        let signature = lowered.binding_signature();
        let pending =
            intent
                .binding
                .with_signature(authored.clone(), move |mut binding, preparation| {
                    binding.check_policies(&authored)?;
                    let target = preparation.declaration();
                    let definitions = target.definitions();
                    if target.records().declaration != template {
                        return Err(conflict(
                            "default preparation target differs from its sealed template",
                        ));
                    }
                    match &binding.requirement_owner {
                        None => {
                            binding.requirement_owner = Some((intent.method, definitions.clone()))
                        }
                        Some((owner, scope))
                            if same_definition(*owner, scope, intent.method, definitions)
                                || same_definition(*owner, scope, template, definitions) => {}
                        Some(_) => {
                            return Err(conflict("foreign native default requirement owner"));
                        }
                    }
                    binding.check(&signature, definitions, preparation.catalog())?;
                    Ok(binding)
                });
        contract.methods[index].implementation =
            CallableImplementation::NativeDefault(lowered.application);
        draft.declaration.functions.push(lowered.template);
        draft.declaration.private_functions.insert(template);
        draft
            .declaration
            .callable_requirements
            .insert(template, lowered.requirements);
        if let Some(result) = lowered.concrete_result {
            draft.declaration.concrete_results.insert(template, result);
        }
        if draft.bindings.insert(template, pending).is_some() {
            return Err(conflict("duplicate default binding target"));
        }
    }
    Ok(())
}

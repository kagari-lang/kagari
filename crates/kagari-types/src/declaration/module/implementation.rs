//! Native implementation construction and checking share the scoped method projection.
use crate::{
    callable::CallableImplementation,
    declaration::{
        FnDecl, TraitDef,
        module::{DeclarationError, ImplDecl, ModuleDecl},
        verify::trait_valid,
    },
    ty::{
        GenericBound, GenericParam, Ty,
        substitution::{TypeSubstitution, resolve_associated_outputs},
    },
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind,
        map::DefinitionContext,
        table::{DefinitionId, DefinitionTable},
    },
};
use std::collections::{BTreeMap, HashSet};

impl ModuleDecl {
    /// Construct a trait implementation in the supplied construction context.
    /// The context owns all inputs; this registers implementation/method names but
    /// does not validate or publish the containing module.
    pub fn implement_trait(
        &mut self,
        contract: &TraitDef,
        mut implementation: ImplDecl,
        bindings: &[(&str, DefinitionId)],
        context: &DefinitionContext,
    ) -> Result<(), DeclarationError> {
        if !implementation.methods.is_empty() {
            return Err(DeclarationError(
                "implementation methods must be supplied through bindings".into(),
            ));
        }
        let invalid = |error| DeclarationError(format!("invalid native identities: {error}"));
        let definitions = self.collect_definitions(context)?;
        let root = definitions
            .lookup_root(&self.identity)
            .ok_or_else(|| DeclarationError("missing module identity".into()))?;
        let occurrence = u32::try_from(self.implementations.len())
            .map_err(|_| DeclarationError("too many implementations".into()))?;
        let owner = context
            .intern_child(root, DefinitionKind::Impl, "", occurrence)
            .map_err(invalid)?;
        for method in &contract.methods {
            context
                .intern_child(owner, DefinitionKind::Method, &method.name, 0)
                .map_err(invalid)?;
        }
        let definitions = context.snapshot();
        implementation.methods =
            implementation_methods(contract, &implementation, owner, bindings, &definitions)?;
        let trait_type = implementation
            .trait_type
            .as_ref()
            .expect("checked trait implementation");
        for method in &contract.methods {
            let source = Self::method_id(trait_type.declaration, &method.name, &definitions)?;
            let target = Self::method_id(owner, &method.name, &definitions)?;
            if let Some(doc) = self.documentation.get(&source).cloned() {
                self.documentation.entry(target).or_insert(doc);
            }
        }
        self.implementations.push(implementation);
        Ok(())
    }

    pub(super) fn validate_implementations(
        &self,
        definitions: &DefinitionTable,
    ) -> Result<(), DeclarationError> {
        let contracts = self
            .traits
            .iter()
            .map(|contract| {
                Ok((
                    self.definition(DefinitionKind::Trait, &contract.name, definitions)?,
                    contract,
                ))
            })
            .collect::<Result<_, DeclarationError>>()?;
        self.check_implementations(&contracts, false, definitions)
    }

    /// Validate method contracts against a complete installed native catalog.
    /// Generic applicability and parent witnesses retain their ordinary checked
    /// HIR/portable proofs; this check does not infer an implementation body.
    pub fn validate_trait_implementations(
        &self,
        contracts: &BTreeMap<DefinitionId, &TraitDef>,
        definitions: &DefinitionTable,
    ) -> Result<(), DeclarationError> {
        self.check_implementations(contracts, true, definitions)
    }

    fn check_implementations(
        &self,
        contracts: &BTreeMap<DefinitionId, &TraitDef>,
        require_external: bool,
        definitions: &DefinitionTable,
    ) -> Result<(), DeclarationError> {
        let invalid =
            || DeclarationError("trait implementation differs from its registered contract".into());
        for (index, implementation) in self.implementations.iter().enumerate() {
            let Some(trait_type) = &implementation.trait_type else {
                continue;
            };
            let Some(contract) = contracts.get(&trait_type.declaration) else {
                if !require_external
                    && definitions
                        .resolve(trait_type.declaration)
                        .map_err(|_| invalid())?
                        .module()
                        != &self.identity
                {
                    continue;
                }
                return Err(invalid());
            };
            let bindings = implementation
                .methods
                .iter()
                .map(|method| {
                    let CallableImplementation::Native(binding) = &method.implementation else {
                        return Err(invalid());
                    };
                    Ok((method.name.as_str(), *binding))
                })
                .collect::<Result<Vec<_>, DeclarationError>>()?;
            let mut expected = implementation_methods(
                contract,
                implementation,
                self.implementation_id(index, definitions)?,
                &bindings,
                definitions,
            )?;
            // Required access is a local native impl fact when the Kagari trait
            // does not impose it. Preserve every other signature/policy check;
            // native declaration and installed binding validation check this fact
            // against the receiver target and the actual lifetime-aware adapter.
            for (expected, actual) in expected.iter_mut().zip(&implementation.methods) {
                if expected.method_policy.receiver_access.is_none() {
                    expected.method_policy.receiver_access = actual.method_policy.receiver_access;
                }
                // A trait's Self signature does not select external-ID storage.
                // The explicit native impl owns its checked passing/effect policy;
                // function and binding validation still require an exact adapter.
                if expected.host_policy.is_none() {
                    expected.host_policy = actual.host_policy.clone();
                }
            }
            if expected != implementation.methods {
                return Err(invalid());
            }
            let mut substitution =
                TypeSubstitution::for_owner(&trait_type.declaration, &trait_type.arguments);
            substitution.bind_receiver(&trait_type.declaration, &implementation.for_type);
            for parent in &contract.supertraits {
                if definitions
                    .resolve(parent.declaration)
                    .map_err(|_| invalid())?
                    .module()
                    != &self.identity
                {
                    continue;
                }
                let cancel = CancellationToken::default();
                let parent = resolve_associated_outputs(
                    &Ty::Trait(parent.clone()),
                    trait_type,
                    &Ty::SelfType(trait_type.declaration),
                    &cancel,
                )
                .map_err(|_| invalid())?;
                let parent = substitution
                    .apply(&parent, &cancel)
                    .map_err(|_| invalid())?;
                let required =
                    self.normalized(&parent, &implementation.generic_params, definitions)?;
                let target = self.normalized(
                    &implementation.for_type,
                    &implementation.generic_params,
                    definitions,
                )?;
                let mut found = false;
                for candidate in &self.implementations {
                    let Some(candidate_trait) = &candidate.trait_type else {
                        continue;
                    };
                    let available = self.normalized(
                        &Ty::Trait(candidate_trait.clone()),
                        &candidate.generic_params,
                        definitions,
                    )?;
                    let parent_matches = matches!((&available, &required),
                        (Ty::Trait(available), Ty::Trait(required))
                        if available.declaration == required.declaration
                            && available.arguments == required.arguments
                            && required.associated_types.iter().all(|(member, output)|
                                available.associated_types.get(member) == Some(output)));
                    if parent_matches
                        && self.normalized(
                            &candidate.for_type,
                            &candidate.generic_params,
                            definitions,
                        )? == target
                    {
                        found = true;
                        break;
                    }
                }
                if !found {
                    return Err(DeclarationError(
                        "missing native supertrait implementation".into(),
                    ));
                }
            }
        }
        Ok(())
    }

    fn normalized(
        &self,
        ty: &Ty,
        parameters: &[GenericParam],
        definitions: &DefinitionTable,
    ) -> Result<Ty, DeclarationError> {
        let owner = self.implementation_id(0, definitions)?;
        let arguments = parameters
            .iter()
            .enumerate()
            .map(|(position, _)| Ty::Parameter { owner, position })
            .collect::<Vec<_>>();
        let mut substitution = TypeSubstitution::default();
        for (parameter, argument) in parameters.iter().zip(&arguments) {
            substitution.bind(&parameter.owner, parameter.position, argument);
        }
        substitution
            .apply(ty, &CancellationToken::default())
            .map_err(|error| DeclarationError(format!("invalid generic template: {error:?}")))
    }
}

fn implementation_methods(
    contract: &TraitDef,
    implementation: &ImplDecl,
    owner: DefinitionId,
    bindings: &[(&str, DefinitionId)],
    definitions: &DefinitionTable,
) -> Result<Vec<FnDecl>, DeclarationError> {
    let trait_type = implementation
        .trait_type
        .as_ref()
        .ok_or_else(|| DeclarationError("missing trait implementation target".into()))?;
    if !trait_valid(
        contract,
        trait_type.declaration,
        &CancellationToken::default(),
        definitions,
    ) {
        return Err(DeclarationError("invalid native trait contract".into()));
    }
    if trait_type.arguments.len() != contract.generic_params.len()
        || trait_type.associated_types.len() != contract.associated_types.len()
        || contract.associated_types.iter().any(|member| {
            !trait_type
                .associated_types
                .contains_key(&member.declaration)
        })
    {
        return Err(DeclarationError(
            "trait arguments or required methods differ".into(),
        ));
    }
    let mut names = HashSet::new();
    if bindings.iter().any(|(name, _)| !names.insert(*name)) {
        return Err(DeclarationError("duplicate method binding".into()));
    }
    if bindings
        .iter()
        .any(|(name, _)| !contract.methods.iter().any(|method| method.name == *name))
    {
        return Err(DeclarationError("unknown trait method binding".into()));
    }
    let cancel = CancellationToken::default();
    let mut methods = vec![];
    for method in &contract.methods {
        let binding = bindings.iter().find(|(name, _)| *name == method.name);
        let Some(binding) = binding else {
            if matches!(
                method.implementation,
                CallableImplementation::NativeDefault(_) | CallableImplementation::Script
            ) {
                continue;
            }
            return Err(DeclarationError(format!("missing method {}", method.name)));
        };
        if !method.method_policy.override_allowed {
            return Err(DeclarationError(format!(
                "method {} forbids overriding",
                method.name
            )));
        }
        let method_owner = ModuleDecl::method_id(owner, &method.name, definitions)?;
        let own_parameters = method
            .generic_params
            .iter()
            .enumerate()
            .map(|(position, _)| GenericParam {
                owner: method_owner,
                position,
            })
            .collect::<Vec<_>>();
        let mut substitution =
            TypeSubstitution::for_owner(&trait_type.declaration, &trait_type.arguments);
        substitution.bind_receiver(&trait_type.declaration, &implementation.for_type);
        let own_types = own_parameters
            .iter()
            .map(GenericParam::as_type)
            .collect::<Vec<_>>();
        for (declared, implemented) in method.generic_params.iter().zip(&own_types) {
            substitution.bind(&declared.owner, declared.position, implemented);
        }
        let resolve = |ty: &Ty| {
            let ty = substitution
                .apply(ty, &cancel)
                .map_err(|error| DeclarationError(format!("invalid substitution: {error:?}")))?;
            resolve_associated_outputs(&ty, trait_type, &implementation.for_type, &cancel)
                .map_err(|error| DeclarationError(format!("invalid associated output: {error:?}")))
        };
        let mut method = method.clone();
        method.implementation = CallableImplementation::Native(binding.1);
        method.generic_params = implementation.generic_params.clone();
        method.generic_params.extend(own_parameters);
        method.bounds = method
            .bounds
            .iter()
            .map(|bound| {
                Ok(GenericBound {
                    ty: resolve(&bound.ty)?,
                    constraints: bound
                        .constraints
                        .iter()
                        .map(|constraint| {
                            let Ty::Trait(interface) = resolve(&Ty::Trait(constraint.clone()))?
                            else {
                                unreachable!("trait substitution");
                            };
                            Ok(interface)
                        })
                        .collect::<Result<_, DeclarationError>>()?,
                })
            })
            .collect::<Result<_, DeclarationError>>()?;
        method.params = method
            .params
            .into_iter()
            .map(|mut param| {
                param.ty = resolve(&param.ty)?;
                Ok(param)
            })
            .collect::<Result<_, DeclarationError>>()?;
        method.return_type = resolve(&method.return_type)?;
        methods.push(method);
    }
    Ok(methods)
}

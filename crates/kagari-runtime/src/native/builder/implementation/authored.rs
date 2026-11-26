//! Lower an impl member from author input, without consulting its trait signature.
use super::{MethodsBuilder, conflict};
use crate::native::{
    adapter::NativeAdapter,
    binding::NativeResult,
    declarations::{FunctionBuilder, FunctionDecl},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionKind, table::DefinitionId},
};
use kagari_types::{
    callable::{CallableImplementation, Signature},
    declaration::{FnDecl, requirement::NativeCallableRequirement},
    ty::Ty,
};
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct AuthoredMethods {
    pub(super) methods: Vec<FnDecl>,
    pub(super) requirements: BTreeMap<DefinitionId, Vec<NativeCallableRequirement>>,
    pub(super) concrete_results: BTreeMap<DefinitionId, Ty>,
}

impl MethodsBuilder<'_> {
    /// Bind an independently declared implementation member. An instance method
    /// declares its first parameter as `self` with the implementing receiver type.
    /// The adapter sees this impl's signature, not the trait's. Its codecs and
    /// policies must agree, and the applied trait is checked separately.
    pub fn define_method<M, R>(
        &mut self,
        declaration: FunctionDecl,
        entry: impl NativeAdapter<M, R>,
    ) -> NativeResult<()> {
        self.define_method_with(declaration, |_| Ok(()), entry)
    }

    /// Configure method-local type parameters, bounds or callable requirements
    /// before preparing the adapter. These parameters belong to the impl member,
    /// not the trait member. Failed configuration or preparation publishes nothing.
    pub fn define_method_with<M, R>(
        &mut self,
        mut declaration: FunctionDecl,
        configure: impl FnOnce(&mut FunctionBuilder<'_>) -> NativeResult<()>,
        entry: impl NativeAdapter<M, R>,
    ) -> NativeResult<()> {
        if !self
            .applied
            .contract
            .contract
            .methods
            .iter()
            .any(|method| method.name == declaration.name)
        {
            return Err(conflict("unknown implementation member"));
        }
        let context = self.catalog.types.context();
        let id = context
            .intern_child(self.id, DefinitionKind::Method, &declaration.name, 0)
            .map_err(conflict)?;
        let documentation = declaration.documentation.take();
        let mut method = declaration.lower(CallableImplementation::Native(id), context)?;
        let mut concrete = BTreeMap::new();
        let mut requirements = Vec::new();
        configure(&mut FunctionBuilder {
            id,
            context,
            concrete_results: &mut concrete,
            signature: &mut method,
            requirements: &mut requirements,
            names: &mut Vec::new(),
        })?;
        let cancel = CancellationToken::default();
        let definitions = context.snapshot();
        let rebind = |ty: &Ty| {
            self.substitution
                .apply(ty, &cancel)
                .map_err(|_| conflict("invalid implementation type substitution"))
        };
        for param in &mut method.params {
            param.ty = rebind(&param.ty)?;
        }
        method.return_type = rebind(&method.return_type)?;
        method.bounds = self
            .substitution
            .apply_bounds(&method.bounds, &definitions, &cancel)
            .map_err(|_| conflict("invalid implementation bounds substitution"))?;
        method
            .generic_params
            .splice(..0, self.parameters.iter().cloned());
        let concrete = concrete.get(&id).map(rebind).transpose()?;
        let requirements = requirements
            .iter()
            .map(|requirement| requirement.apply(self.substitution, &cancel))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| conflict("invalid implementation requirement substitution"))?;
        if let Some(policy) = self.host_policies.get(&method.name)
            && method
                .host_policy
                .as_ref()
                .is_some_and(|declared| declared != policy)
        {
            return Err(conflict("conflicting implementation host policy"));
        }
        self.apply_policies(&mut method);
        let binding = self.prepare(&method, entry)?;
        binding.check(
            &Signature {
                params: method.params.iter().map(|param| param.ty.clone()).collect(),
                result: concrete.as_ref().unwrap_or(&method.return_type).clone(),
            },
            &definitions,
        )?;
        if let Some(text) = documentation {
            self.documentation.insert(method.name.clone(), text);
        }
        if let Some(concrete) = concrete {
            self.authored.concrete_results.insert(id, concrete);
        }
        if !requirements.is_empty() {
            self.authored.requirements.insert(id, requirements);
        }
        self.bindings.insert(method.name.clone(), binding);
        self.authored.methods.push(method);
        Ok(())
    }
}

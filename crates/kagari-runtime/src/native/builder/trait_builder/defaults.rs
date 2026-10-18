//! Lower declared defaults to ordinary private native templates.
use crate::{
    error::RuntimeError,
    native::{
        binding::NativeResult,
        builder::{
            default_lowering::{DefaultTemplateSpec, lower_default, trait_default_assumptions},
            trait_builder::TraitBuilder,
        },
    },
};
use kagari_common::{cancellation::CancellationToken, identity::DefinitionKind};
use kagari_types::{
    callable::CallableImplementation,
    declaration::module::ModuleDecl,
    ty::{GenericParam, Ty},
};

impl TraitBuilder<'_> {
    pub(super) fn lower_defaults(&mut self) -> NativeResult<()> {
        let cancel = CancellationToken::default();
        let receiver = Ty::SelfType(self.id);
        let assumptions = trait_default_assumptions(self.id, &self.declaration);
        let mut lowered = vec![];
        for (index, method) in self.declaration.methods.iter().enumerate() {
            let method_id =
                ModuleDecl::method_id(self.id, &method.name, &self.module.providers.definitions())
                    .map_err(|error| RuntimeError::metadata_conflict(error.to_string()))?;
            let Some(binding) = self.defaults.get(&method_id) else {
                if self
                    .requirements
                    .get(&method_id)
                    .is_some_and(|requirements| !requirements.is_empty())
                    || self.concrete_results.contains_key(&method_id)
                {
                    return Err(RuntimeError::metadata_conflict(
                        "required method has native default metadata but no body",
                    ));
                }
                continue;
            };
            let name = format!(
                "__default_{}_{}_{}",
                self.declaration.name.len(),
                self.declaration.name,
                method.name
            );
            let id = self.module.definition(DefinitionKind::Function, &name)?;
            if self
                .module
                .declaration
                .functions
                .iter()
                .any(|function| function.name == name)
            {
                return Err(RuntimeError::metadata_conflict(
                    "duplicate native default template",
                ));
            }
            let arguments: Vec<_> = [receiver.clone()]
                .into_iter()
                .chain(
                    self.declaration
                        .generic_params
                        .iter()
                        .chain(&method.generic_params)
                        .map(GenericParam::as_type),
                )
                .collect();
            let default = lower_default(
                DefaultTemplateSpec {
                    method,
                    template: id,
                    arguments: &arguments,
                    assumptions: &assumptions,
                    requirements: self.requirements.get(&method_id).map_or(&[], Vec::as_slice),
                    concrete_result: self.concrete_results.get(&method_id),
                },
                &self.module.providers.definitions(),
                &cancel,
            )
            .map_err(|error| RuntimeError::metadata_conflict(error.to_string()))?;
            binding.check(
                &default.binding_signature(),
                &self.module.providers.definitions(),
                &self.module.providers,
            )?;
            lowered.push((index, default, binding.clone()));
        }
        for (index, default, binding) in lowered {
            let id = default.application.declaration;
            if let Some(ty) = default.concrete_result {
                self.module.declaration.concrete_results.insert(id, ty);
            }
            self.declaration.methods[index].implementation =
                CallableImplementation::NativeDefault(default.application);
            self.module.declaration.functions.push(default.template);
            self.module.declaration.private_functions.insert(id);
            self.module
                .declaration
                .callable_requirements
                .insert(id, default.requirements);
            self.module.bindings.insert(id, binding);
        }
        Ok(())
    }
}

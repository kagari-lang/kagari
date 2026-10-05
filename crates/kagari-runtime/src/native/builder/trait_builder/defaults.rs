//! Lower declared defaults to ordinary private native templates.
use crate::{
    error::RuntimeError,
    native::{
        binding::NativeResult, builder::trait_builder::TraitBuilder, declarations::normalize_bounds,
    },
};
use kagari_common::{cancellation::CancellationToken, identity::DefinitionKind};
use kagari_types::{
    callable::{CallableImplementation, NativeDefaultApplication, Signature},
    declaration::module::ModuleDecl,
    ty::{Constraint, GenericBound, GenericParam, NominalTy, Ty, substitution::TypeSubstitution},
};
use std::collections::BTreeMap;

impl TraitBuilder<'_> {
    pub(super) fn lower_defaults(&mut self) -> NativeResult<()> {
        let cancel = CancellationToken::default();
        let invalid = |_| RuntimeError::metadata_conflict("invalid native default substitution");
        let receiver = Ty::SelfType(self.id.clone());
        let interface = NominalTy {
            declaration: self.id.clone(),
            arguments: self
                .declaration
                .generic_params
                .iter()
                .map(GenericParam::as_type)
                .collect(),
            associated_types: BTreeMap::new(),
        };
        let base = interface.clone();
        let mut assumptions = self.declaration.bounds.clone();
        for output in &self.declaration.associated_types {
            let projection = Ty::Projection {
                receiver: Box::new(receiver.clone()),
                interface: Box::new(base.clone()),
                member: output.declaration.clone(),
                arguments: vec![],
            };
            // The receiver bound supplies its associated outputs. Equating an
            // output to its own projection makes generated source recursive.
            if !output.bounds.is_empty() {
                assumptions.push(GenericBound {
                    ty: projection,
                    constraints: output.bounds.clone(),
                });
            }
        }
        assumptions.push(GenericBound {
            ty: receiver.clone(),
            constraints: vec![Constraint::Trait(interface)],
        });
        let mut lowered = vec![];
        for (index, method) in self.declaration.methods.iter().enumerate() {
            let method_id = ModuleDecl::method_id(&self.id, &method.name);
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
            let id = self
                .module
                .declaration
                .definition(DefinitionKind::Function, &name);
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
            let parameters: Vec<_> = (0..arguments.len())
                .map(|position| GenericParam {
                    owner: id.clone(),
                    position,
                })
                .collect();
            let types: Vec<_> = parameters.iter().map(GenericParam::as_type).collect();
            let mut substitution = TypeSubstitution::default();
            substitution.bind_receiver(&self.id, &types[0]);
            for (parameter, ty) in self
                .declaration
                .generic_params
                .iter()
                .chain(&method.generic_params)
                .zip(&types[1..])
            {
                substitution.bind(&parameter.owner, parameter.position, ty);
            }
            let mut template = method.clone();
            template.name = name;
            template.implementation = CallableImplementation::Native(id.clone());
            template.generic_params = parameters;
            let mut bounds = assumptions.clone();
            bounds.extend(method.bounds.clone());
            template.bounds = substitution
                .apply_bounds(&bounds, &cancel)
                .map_err(invalid)?;
            normalize_bounds(&mut template);
            for parameter in &mut template.params {
                if parameter.name == "self" {
                    parameter.name = "__receiver".into();
                }
                parameter.ty = substitution
                    .apply(&parameter.ty, &cancel)
                    .map_err(invalid)?;
            }
            template.return_type = substitution
                .apply(&template.return_type, &cancel)
                .map_err(invalid)?;
            let requirements = self
                .requirements
                .get(&method_id)
                .into_iter()
                .flatten()
                .map(|requirement| requirement.apply(&substitution, &cancel))
                .collect::<Result<Vec<_>, _>>()
                .map_err(invalid)?;
            let concrete_result = self
                .concrete_results
                .get(&method_id)
                .map(|ty| substitution.apply(ty, &cancel))
                .transpose()
                .map_err(invalid)?;
            binding.check(
                &Signature {
                    params: template
                        .params
                        .iter()
                        .map(|parameter| parameter.ty.clone())
                        .collect(),
                    result: concrete_result
                        .clone()
                        .unwrap_or_else(|| template.return_type.clone()),
                },
                &self.module.providers,
            )?;
            lowered.push((
                index,
                id,
                arguments,
                template,
                requirements,
                concrete_result,
                binding.clone(),
            ));
        }
        for (index, id, arguments, template, requirements, concrete_result, binding) in lowered {
            if let Some(ty) = concrete_result {
                self.module
                    .declaration
                    .concrete_results
                    .insert(id.clone(), ty);
            }
            self.declaration.methods[index].implementation =
                CallableImplementation::NativeDefault(NativeDefaultApplication {
                    declaration: id.clone(),
                    arguments,
                });
            self.module.declaration.functions.push(template);
            self.module.declaration.private_functions.insert(id.clone());
            self.module
                .declaration
                .callable_requirements
                .insert(id.clone(), requirements);
            self.module.bindings.insert(id, binding);
        }
        Ok(())
    }
}

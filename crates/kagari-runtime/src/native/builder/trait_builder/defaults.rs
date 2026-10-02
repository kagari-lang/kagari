//! Lower declared defaults to ordinary private native templates.
use crate::{
    error::RuntimeError,
    native::{
        binding::NativeResult, builder::trait_builder::TraitBuilder, declarations::normalize_bounds,
    },
};
use kagari_abi::{
    callable::{CallableImplementation, NativeDefaultApplication},
    declaration::ModuleDecl,
    native_import::NativeSignature,
    types::{
        AbiType, ConstraintAbi, GenericBoundAbi, GenericParameterAbi, NominalAbiType,
        substitution::TypeSubstitution,
    },
};
use kagari_common::{cancellation::CancellationToken, identity::DefinitionKind};
use std::collections::BTreeMap;

impl TraitBuilder<'_> {
    pub(super) fn lower_defaults(&mut self) -> NativeResult<()> {
        let cancel = CancellationToken::default();
        let invalid = |_| RuntimeError::metadata_conflict("invalid native default substitution");
        let receiver = AbiType::SelfType(self.id.clone());
        let mut interface = NominalAbiType {
            declaration: self.id.clone(),
            arguments: self
                .declaration
                .generic_params
                .iter()
                .map(GenericParameterAbi::as_type)
                .collect(),
            associated_types: BTreeMap::new(),
        };
        let base = interface.clone();
        let mut assumptions = self.declaration.bounds.clone();
        for output in &self.declaration.associated_types {
            let projection = AbiType::Projection {
                receiver: Box::new(receiver.clone()),
                interface: Box::new(base.clone()),
                member: output.declaration.clone(),
                arguments: vec![],
            };
            interface
                .associated_types
                .insert(output.declaration.clone(), projection.clone());
            if !output.bounds.is_empty() {
                assumptions.push(GenericBoundAbi {
                    ty: projection,
                    constraints: output.bounds.clone(),
                });
            }
        }
        assumptions.push(GenericBoundAbi {
            ty: receiver.clone(),
            constraints: vec![ConstraintAbi::Trait(interface)],
        });
        let mut lowered = vec![];
        for (index, method) in self.declaration.methods.iter().enumerate() {
            let method_id = ModuleDecl::method_id(&self.id, &method.name);
            let Some(binding) = self.defaults.get(&method_id) else {
                if self
                    .requirements
                    .get(&method_id)
                    .is_some_and(|requirements| !requirements.is_empty())
                {
                    return Err(RuntimeError::metadata_conflict(
                        "required method has native default operations but no body",
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
                        .map(GenericParameterAbi::as_type),
                )
                .collect();
            let parameters: Vec<_> = (0..arguments.len())
                .map(|position| GenericParameterAbi {
                    owner: id.clone(),
                    position,
                })
                .collect();
            let types: Vec<_> = parameters
                .iter()
                .map(GenericParameterAbi::as_type)
                .collect();
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
            binding.check(
                &NativeSignature {
                    params: template
                        .params
                        .iter()
                        .map(|parameter| parameter.ty.clone())
                        .collect(),
                    result: template.return_type.clone(),
                },
                &self.module.providers,
            )?;
            lowered.push((
                index,
                id,
                arguments,
                template,
                requirements,
                binding.clone(),
            ));
        }
        for (index, id, arguments, template, requirements, binding) in lowered {
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

//! Materialize native method contracts while the checked source catalog is available.
use crate::source::lower::{MirLoweringError, instances::InstancePlanner};
use kagari_common::{identity::table::DefinitionId, span::Span};
use kagari_contract::{
    callable::generic::GenericBody, native_import::NativeImport, types::ConcreteFunctionIdentity,
};
use kagari_hir::{
    resolver::resolved::ResolvedName,
    types::{TypeId, semantic::raise_type},
};
use kagari_types::{
    callable::{CallableImplementation, Signature},
    ty::{GenericParam, Ty, substitution::TypeSubstitution},
};

impl InstancePlanner<'_> {
    pub(crate) fn prepare_native_target(
        &mut self,
        declaration: &DefinitionId,
        arguments: &[TypeId],
        span: Span,
    ) -> Result<bool, MirLoweringError> {
        let Some(declared) = self.registered_native_declaration(declaration).cloned() else {
            return self.prepare_default_target(declaration, arguments, span);
        };
        let invalid = || MirLoweringError::MissingBinding("checked native method instance");
        // All declared parameters must be supplied, either concretely or by a
        // shared entry's explicitly scoped method binders.
        if declared.function.generic_params.len() != arguments.len() {
            return Ok(false);
        }
        let mut import = self.native_target_import(declaration, arguments, span)?;
        let instance = import.instance.clone();
        let parent = self
            .definitions()
            .parent(*declaration)
            .ok()
            .flatten()
            .ok_or_else(invalid)?;
        if let Some(implementation) = self.catalog.implementation_signature(&parent) {
            let count = implementation.generic_params.len();
            if arguments.len() < count {
                return Err(invalid());
            }
            self.record_interface(&parent, &arguments[..count], span)?;
        }
        if self
            .native_targets
            .iter()
            .any(|target| target.instance == instance)
        {
            return Ok(true);
        }
        if !arguments.is_empty() {
            self.charge_layout_instance(span)?;
        }
        import.callables = self.native_callables(&mut import, span)?;
        if !import.structurally_valid(&self.definitions()) {
            return Err(invalid());
        }
        self.native_targets.push(import);
        Ok(true)
    }

    pub(super) fn native_target_import(
        &self,
        declaration: &DefinitionId,
        arguments: &[TypeId],
        span: Span,
    ) -> Result<NativeImport, MirLoweringError> {
        let declared = self.registered_native_declaration(declaration).ok_or(
            MirLoweringError::MissingBinding("registered native template"),
        )?;
        if declared.function.generic_params.len() != arguments.len() {
            return Err(MirLoweringError::MissingBinding(
                "native template arguments",
            ));
        }
        let invalid = || MirLoweringError::MissingBinding("checked native template application");
        let CallableImplementation::Native(binding) = &declared.function.implementation else {
            return Err(invalid());
        };
        let arguments = self.arguments(arguments, &Default::default(), span)?;
        let instance = ConcreteFunctionIdentity {
            declaration: *declaration,
            arguments: arguments.iter().map(|ty| self.encode_type(ty)).collect(),
        };
        let parameters = self.native_parameters(&instance.arguments)?;
        let mut substitution = TypeSubstitution::default();
        for (param, argument) in declared
            .function
            .generic_params
            .iter()
            .zip(&instance.arguments)
        {
            substitution.bind(&param.owner, param.position, argument);
        }
        let normalize = |ty| {
            let ty = substitution
                .apply(ty, &self.options.cancel)
                .map_err(|_| invalid())?;
            Ok(self.encode_type(
                &self
                    .catalog
                    .normalize_type(&self.definitions(), &raise_type(&ty)),
            ))
        };
        let requirements = substitution
            .apply_bounds(
                &declared.function.bounds,
                &self.definitions(),
                &self.options.cancel,
            )
            .map_err(|_| invalid())?;
        let import = NativeImport {
            receiver_access: declared.function.method_policy.receiver_access,
            result_adapter: None,
            generic: (!parameters.is_empty()).then(|| GenericBody {
                parameters,
                bounds: requirements.clone(),
            }),
            signature: Signature {
                params: declared
                    .function
                    .params
                    .iter()
                    .map(|param| normalize(&param.ty))
                    .collect::<Result<_, MirLoweringError>>()?,
                result: normalize(&declared.function.return_type)?,
            },
            requirements,
            instance,
            binding: *binding,
            callables: vec![],
        };
        Ok(import)
    }

    fn native_parameters(&self, arguments: &[Ty]) -> Result<Vec<GenericParam>, MirLoweringError> {
        let mut pending: Vec<_> = arguments.iter().collect();
        let mut parameters = vec![];
        let mut remaining = self.options.max_type_nodes;
        while let Some(ty) = pending.pop() {
            self.check()?;
            remaining = remaining
                .checked_sub(1)
                .ok_or(MirLoweringError::UnsupportedExpr(
                    "native binder scope exceeds type node limit",
                ))?;
            match ty {
                Ty::Parameter { owner, position } => {
                    let parameter = GenericParam {
                        owner: *owner,
                        position: *position,
                    };
                    if !parameters
                        .iter()
                        .any(|parameter: &GenericParam| parameter.owner == *owner)
                    {
                        parameters.extend(self.native_parameter_scope(*owner)?);
                    }
                    if !parameters.contains(&parameter) {
                        return Err(MirLoweringError::MissingBinding(
                            "native application binder",
                        ));
                    }
                }
                Ty::Reference { target, .. } => pending.push(target),
                Ty::Struct(nominal)
                | Ty::Enum(nominal)
                | Ty::NativeObject(nominal)
                | Ty::Trait(nominal) => {
                    pending.extend(&nominal.arguments);
                    pending.extend(nominal.associated_types.values());
                }
                Ty::Projection {
                    receiver,
                    interface,
                    arguments,
                    ..
                } => {
                    pending.push(receiver);
                    pending.extend(&interface.arguments);
                    pending.extend(interface.associated_types.values());
                    pending.extend(arguments);
                }
                Ty::Tuple(items) => pending.extend(items),
                Ty::Function { params, result } => {
                    pending.extend(params);
                    pending.push(result);
                }
                Ty::Array(item) | Ty::Set(item, _) | Ty::Iter(item) | Ty::Range(item, _) => {
                    pending.push(item);
                }
                Ty::Map { key, value, .. } => pending.extend([key.as_ref(), value.as_ref()]),
                Ty::Builtin(_) | Ty::Host(_) | Ty::SelfType(_) => {}
            }
        }
        // An application may mention only P1 inside a tuple or container. Retain
        // its owner's complete scope, including unused P0, without rebinding P1.
        parameters.sort_by_key(|parameter| parameter.position);
        Ok(parameters)
    }

    fn native_parameter_scope(
        &self,
        owner: DefinitionId,
    ) -> Result<Vec<GenericParam>, MirLoweringError> {
        let invalid = || MirLoweringError::MissingBinding("native application binder owner");
        let definitions = self.definitions();
        let count = if let Some((implementation, method)) =
            self.catalog.default_method(&definitions, &owner)
        {
            let contract = self.catalog.trait_(&method.owner).ok_or_else(invalid)?;
            // Selected defaults rebind the trait's method parameters after the
            // implementation parameters, not after the trait's applied arguments.
            implementation.generic_params.len()
                + method
                    .generic_params
                    .len()
                    .checked_sub(contract.generic_params.len())
                    .ok_or_else(invalid)?
        } else if let Some(native) = self.registered_native_declaration(&owner) {
            native.function.generic_params.len()
        } else if let Some(module) = self.modules.get(&self.module_handle(owner))
            && let Some(ResolvedName::Function(id)) = module.declarations.definition_target(&owner)
        {
            module
                .typed
                .functions
                .iter()
                .find(|function| function.id == id)
                .ok_or_else(invalid)?
                .generic_params
                .len()
        } else if let Some(implementation) = self.catalog.implementation_signature(&owner) {
            implementation.generic_params.len()
        } else if let Some(bridge) = self
            .interface_bridges
            .iter()
            .find(|bridge| bridge.declaration == owner)
        {
            return Ok(bridge.parameters.clone());
        } else {
            return Err(invalid());
        };
        // Shared entries canonically own their declaration's ordered binders;
        // source signatures can still refer to enclosing declaration owners.
        Ok((0..count)
            .map(|position| GenericParam { owner, position })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use crate::tests::common;
    use kagari_types::ty::{GenericParam, Ty};

    #[test]
    fn nested_native_application_retains_unused_leading_binder() {
        for source in [
            r#"
use std::collections::List;
trait Builder {
    fn pack<A, B>(self, first: A, second: B) -> List<[B]>;
}
struct Factory {}
impl Builder for Factory {
    fn pack<A, B>(self, first: A, second: B) -> List<[B]> {
        Vec::from([[second]])
    }
}
fn main() -> Builder { Factory {} }
"#,
            r#"
use std::collections::List;
trait Builder<Outer> {
    fn pack<A, B>(self, first: A, second: B) -> List<[B]> {
        Vec::from([[second]])
    }
}
struct Factory {}
impl Builder<i32> for Factory {}
fn main() -> Builder<i32> { Factory {} }
"#,
            r#"
use std::collections::List;
fn pack<A, B>(first: A, second: B) -> List<[B]> { Vec::from([[second]]) }
trait Builder {
    fn build<A, B>(self, first: A, second: B) -> List<[B]> {
        pack(first, second)
    }
}
struct Factory {}
impl Builder for Factory {}
fn main() -> Builder { Factory {} }
"#,
        ] {
            let program = common::bytecode_ok(source);
            let definitions = program.definitions();
            let mut nested = 0;
            for import in program
                .records()
                .modules
                .iter()
                .flat_map(|module| &module.native_imports)
            {
                let [Ty::Array(element)] = import.instance.arguments.as_slice() else {
                    continue;
                };
                let Ty::Parameter { owner, position: 1 } = element.as_ref() else {
                    continue;
                };
                assert_eq!(
                    definitions.resolve(*owner).unwrap().last().unwrap().name,
                    "pack"
                );
                assert_eq!(
                    import.generic.as_ref().unwrap().parameters,
                    [
                        GenericParam {
                            owner: *owner,
                            position: 0
                        },
                        GenericParam {
                            owner: *owner,
                            position: 1
                        }
                    ]
                );
                assert!(import.structurally_valid(definitions));
                nested += 1;
            }
            assert!(
                nested > 0,
                "expected native applications containing only Array(P1)"
            );
        }
    }
}

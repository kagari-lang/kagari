//! Closed type arguments retain the lexical scope that supplied their layouts.
use kagari_common::identity::{
    mapping::{DefinitionMapper, DefinitionMappingError, DefinitionRecord},
    reference::DefinitionReference,
    table::{DefinitionId, DefinitionTable},
};
#[cfg(test)]
mod identity_tests;
use crate::{
    Runtime,
    error::RuntimeError,
    frame::types::{bindings::TypeBindings, compatibility::TypeView},
    gc::GcHeap,
    module::LoadedModule,
    value::Value,
    value_check::matches_type_in,
};
use kagari_types::{declaration::verify::types_in_scope_in, ty::Ty};
use std::{rc::Rc, slice};

#[derive(Debug, Clone)]
pub struct TypeArgument {
    ty: Ty<DefinitionId>,
    definitions: DefinitionTable,
    origin: Option<Rc<TypeOrigin>>,
}

#[derive(Debug)]
struct TypeOrigin {
    expression: Ty<DefinitionId>,
    scope: Rc<TypeScope>,
}

#[derive(Debug)]
struct TypeScope {
    owner: LoadedModule,
    environment: Option<Rc<TypeBindings>>,
}

#[derive(Debug)]
pub(crate) struct ScopedSignature {
    pub(crate) params: Vec<TypeArgument>,
    pub(crate) result: TypeArgument,
}

impl TypeArgument {
    pub(crate) fn view<'a>(&'a self, owner: &'a LoadedModule) -> TypeView<'a> {
        match &self.origin {
            Some(origin) => TypeView::new(
                &origin.expression,
                &origin.scope.owner,
                origin.scope.environment.as_deref(),
            ),
            None => TypeView::new(&self.ty, owner, None),
        }
    }

    pub(crate) fn matches_heap(&self, heap: &GcHeap, value: &Value, owner: &LoadedModule) -> bool {
        let view = self.view(owner);
        matches_type_in(heap, value, view.ty, view.owner, view.environment)
    }

    pub(crate) fn derive(
        &self,
        runtime: &Runtime,
        fallback: &LoadedModule,
        derive: impl FnOnce(&Ty<DefinitionId>) -> Option<Ty<DefinitionId>>,
    ) -> Result<Self, RuntimeError> {
        let (expression, owner, environment) = match &self.origin {
            Some(origin) => (
                &origin.expression,
                &origin.scope.owner,
                origin.scope.environment.clone(),
            ),
            None => (&self.ty, fallback, None),
        };
        let ty = derive(expression)
            .ok_or_else(|| RuntimeError::module_validation("derived type expression"))?;
        runtime
            .type_arguments(owner, environment, slice::from_ref(&ty))?
            .pop()
            .ok_or_else(|| RuntimeError::module_validation("derived type scope"))
    }

    pub(crate) fn parameter(
        &self,
        runtime: &Runtime,
        fallback: &LoadedModule,
        index: usize,
    ) -> Result<Self, RuntimeError> {
        self.derive(runtime, fallback, |ty| type_parameter(ty, index).cloned())
    }

    pub fn ty(&self) -> &Ty<DefinitionId> {
        &self.ty
    }

    pub fn definitions(&self) -> &DefinitionTable {
        &self.definitions
    }

    pub(crate) fn has_origin(&self) -> bool {
        self.origin.is_some()
    }

    pub(crate) fn validate(&self, runtime: &Runtime) -> Result<(), RuntimeError> {
        let table = runtime.definition_context().snapshot();
        self.ty
            .visit_definitions(
                &mut |id| {
                    self.definitions.resolve(*id)?;
                    table.resolve(*id)?;
                    Ok(())
                },
                &Default::default(),
            )
            .map_err(|error| RuntimeError::module_validation(error.to_string()))?;
        if let Some(origin) = &self.origin
            && !origin.scope.owner.belongs_to(runtime.host.owner())
        {
            return Err(RuntimeError::module_validation(
                "foreign type argument scope",
            ));
        }
        Ok(())
    }

    pub(crate) fn matches(&self, runtime: &Runtime, value: &Value, owner: &LoadedModule) -> bool {
        if let Some(origin) = &self.origin {
            runtime.matches_type_in(
                value,
                &origin.expression,
                &origin.scope.owner,
                origin.scope.environment.as_deref(),
            )
        } else {
            runtime.matches_interface_method_abi(value, &self.ty, owner)
        }
    }
}

pub(crate) fn type_parameter(ty: &Ty<DefinitionId>, index: usize) -> Option<&Ty<DefinitionId>> {
    match ty {
        Ty::Struct(nominal)
        | Ty::Enum(nominal)
        | Ty::NativeObject(nominal)
        | Ty::Trait(nominal) => nominal.arguments.get(index),
        Ty::Tuple(items) => items.get(index),
        Ty::Array(item, _) | Ty::Set(item, _) | Ty::Iter(item) | Ty::Range(item, _)
            if index == 0 =>
        {
            Some(item)
        }
        Ty::Map { key, value, .. } => match index {
            0 => Some(key),
            1 => Some(value),
            _ => None,
        },
        _ => None,
    }
}

fn contains_nominal_layout(ty: &Ty<DefinitionId>) -> bool {
    let mut pending = vec![ty];
    while let Some(ty) = pending.pop() {
        match ty {
            Ty::Struct(_) | Ty::Enum(_) => return true,
            Ty::Tuple(types) => pending.extend(types),
            Ty::Function { params, result } => {
                pending.extend(params);
                pending.push(result);
            }
            Ty::NativeObject(ty) | Ty::Trait(ty) => {
                pending.extend(&ty.arguments);
                pending.extend(ty.associated_types.values());
            }
            Ty::Array(ty, _) | Ty::Iter(ty) | Ty::Range(ty, _) | Ty::Set(ty, _) => pending.push(ty),
            Ty::Map { key, value, .. } => pending.extend([key.as_ref(), value.as_ref()]),
            _ => {}
        }
    }
    false
}

impl Runtime {
    pub(crate) fn matches_capture_type(
        &self,
        value: &Value,
        ty: &Ty<DefinitionId>,
        owner: &LoadedModule,
        environment: &TypeBindings,
    ) -> bool {
        // A mutable capture carries a cell handle; its semantic type describes
        // the contained variable rather than that internal storage handle.
        match value {
            Value::Cell(id) => self
                .gc
                .captured_cell_value(*id)
                .is_some_and(|value| self.matches_type_in(&value, ty, owner, Some(environment))),
            _ => self.matches_type_in(value, ty, owner, Some(environment)),
        }
    }

    /// Prepare concrete host-supplied arguments against a specific loaded generation.
    pub fn resolve_type_arguments<I: DefinitionReference>(
        &self,
        owner: &LoadedModule,
        types: &[Ty<I>],
    ) -> Result<Vec<TypeArgument>, RuntimeError> {
        let cancel = Default::default();
        let types = types
            .iter()
            .map(|ty| {
                ty.map_identities(&mut DefinitionMapper::new(
                    &mut |id| {
                        id.resolve(owner.definitions())
                            .map_err(DefinitionMappingError::from)
                    },
                    &cancel,
                ))
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| RuntimeError::module_validation(error.to_string()))?;
        self.type_arguments(owner, None, &types)
    }

    pub(crate) fn type_arguments(
        &self,
        owner: &LoadedModule,
        environment: Option<Rc<TypeBindings>>,
        types: &[Ty<DefinitionId>],
    ) -> Result<Vec<TypeArgument>, RuntimeError> {
        let invalid = || RuntimeError::module_validation("type argument scope");
        // Reified types own immutable metadata, not executable module instances.
        if !owner.belongs_to(self.host.owner()) {
            return Err(invalid());
        }
        let mut scope = None;
        let definitions = self.definition_context().snapshot();
        let mut arguments = Vec::with_capacity(types.len());
        for expression in types {
            if let Ty::Parameter { owner, position } = expression {
                let argument = environment
                    .as_ref()
                    .and_then(|environment| environment.argument(owner, *position))
                    .ok_or_else(invalid)?;
                argument.validate(self)?;
                arguments.push(argument.clone());
                continue;
            }
            let ty = match &environment {
                Some(environment) => environment.resolve(expression)?,
                None => expression.clone(),
            };
            if !ty.is_concrete()
                || !types_in_scope_in([&ty], &[], &Default::default(), Some(&definitions))
            {
                return Err(invalid());
            }
            let origin = if contains_nominal_layout(&ty) {
                let scope = match &scope {
                    Some(scope) => Rc::clone(scope),
                    None => {
                        let prepared = Rc::new(TypeScope {
                            owner: owner.clone(),
                            environment: environment.clone(),
                        });
                        scope = Some(prepared.clone());
                        prepared
                    }
                };
                Some(Rc::new(TypeOrigin {
                    expression: expression.clone(),
                    scope,
                }))
            } else {
                None
            };
            arguments.push(TypeArgument {
                ty,
                definitions: definitions.clone(),
                origin,
            });
        }
        Ok(arguments)
    }

    pub(crate) fn matches_type_in(
        &self,
        value: &Value,
        ty: &Ty<DefinitionId>,
        owner: &LoadedModule,
        environment: Option<&TypeBindings>,
    ) -> bool {
        if !self.gc.validate_value(value) {
            return false;
        }
        if let Ty::Parameter {
            owner: binder,
            position,
        } = ty
        {
            return environment
                .and_then(|environment| environment.argument(binder, *position))
                .is_some_and(|argument| argument.matches(self, value, owner));
        }
        if let (Value::Tuple(values), Ty::Tuple(types)) = (value, ty) {
            return values.len() == types.len()
                && values
                    .iter()
                    .zip(types)
                    .all(|(value, ty)| self.matches_type_in(value, ty, owner, environment));
        }
        if matches!(ty, Ty::Host(_)) {
            return self.matches_interface_method_abi(value, ty, owner);
        }
        matches_type_in(&self.gc, value, ty, owner, environment)
    }
}

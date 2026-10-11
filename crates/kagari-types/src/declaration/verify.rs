//! Bounded declaration shape and binder checks, independent of linked execution.
use crate::{
    callable::CallableImplementation,
    declaration::{FnDecl, NativeDeclaration, TraitDef, TypeDef, TypeDefKind},
    host_interface::validate_host_type_identity_in,
    range::RangeKind,
    scalar::BuiltinType,
    ty::{Constraint, GenericBound, GenericParam, Ty},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind, ModuleIdentity,
        reference::DefinitionReference,
        table::{DefinitionId, DefinitionTable},
    },
};
use std::{
    collections::{BTreeMap, HashSet},
    iter,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeclarationValidationError {
    Invalid,
    Cancelled,
}
pub type Parameters = HashSet<(DefinitionId, usize)>;

fn scalar_const_type<I>(ty: &Ty<I>) -> bool {
    matches!(
        ty,
        Ty::Builtin(BuiltinType::Unit | BuiltinType::Bool | BuiltinType::I32 | BuiltinType::F32)
    )
}

pub fn scalar_const_valid<I>(ty: &Ty<I>, value: &str) -> bool {
    match ty {
        Ty::Builtin(BuiltinType::Unit) => value == "const-v1:unit",
        Ty::Builtin(BuiltinType::Bool) => {
            matches!(value, "const-v1:bool:0" | "const-v1:bool:1")
        }
        Ty::Builtin(BuiltinType::I32) => value
            .strip_prefix("const-v1:i32:")
            .and_then(|value| value.parse::<i32>().ok().map(|n| n.to_string() == value))
            .unwrap_or(false),
        Ty::Builtin(BuiltinType::F32) => value
            .strip_prefix("const-v1:f32:")
            .and_then(|value| {
                u32::from_str_radix(value, 16)
                    .ok()
                    .map(|bits| format!("{bits:08x}") == value)
            })
            .unwrap_or(false),
        _ => false,
    }
}

pub fn concrete_type_valid(
    ty: &Ty<DefinitionId>,
    cancel: &CancellationToken,
    table: &DefinitionTable,
) -> bool {
    type_valid(ty, &Parameters::new(), None, cancel, table)
}

/// Validate declaration binders before any concrete native import can use them.
/// The import separately matches this template and the engine's storage guard.
/// All IDs must belong to `table`; constraints use canonical definition content
/// for ordering, so allocation order cannot change declaration validity.
pub fn validate_native_declarations(
    declarations: &[NativeDeclaration<DefinitionId>],
    module: &ModuleIdentity,
    table: &DefinitionTable,
    cancel: &CancellationToken,
) -> Result<(), DeclarationValidationError> {
    let mut identities = HashSet::new();
    for declaration in declarations {
        cancel
            .check()
            .map_err(|_| DeclarationValidationError::Cancelled)?;
        let id = &declaration.declaration;
        let function = &declaration.function;
        let view = table
            .resolve(*id)
            .map_err(|_| DeclarationValidationError::Invalid)?;
        if view.module() != module
            || !identities.insert(id)
            || !view.last().is_some_and(|part| {
                matches!(part.kind, DefinitionKind::Function | DefinitionKind::Method)
                    && part.name == function.name
            })
            || !matches!(
                &function.implementation,
                CallableImplementation::Native(binding)
                    if binding.describe(Some(table)).is_ok_and(|view|
                        view.nominal(DefinitionKind::Function)
                            || view.nominal(DefinitionKind::Method))
            )
        {
            return Err(DeclarationValidationError::Invalid);
        }
        let mut owners = BTreeMap::<_, Vec<_>>::new();
        let mut params = HashSet::new();
        for parameter in &function.generic_params {
            if table.contains(parameter.owner, *id) != Ok(true)
                || !params.insert((parameter.owner, parameter.position))
            {
                return Err(DeclarationValidationError::Invalid);
            }
            owners
                .entry(&parameter.owner)
                .or_default()
                .push(parameter.position);
        }
        if owners
            .values()
            .any(|positions| positions.iter().copied().ne(0..positions.len()))
        {
            return Err(DeclarationValidationError::Invalid);
        }
        let receiver = table
            .parent(*id)
            .map_err(|_| DeclarationValidationError::Invalid)?;
        let self_owner = receiver.as_ref().filter(|id| {
            table
                .resolve(**id)
                .ok()
                .and_then(|view| view.last())
                .is_some_and(|part| part.kind == DefinitionKind::Trait)
        });
        let valid = |ty: &Ty<DefinitionId>| {
            ty.within_wire_limits() && type_valid_in(ty, &params, self_owner, cancel, Some(table))
        };
        let before = |left: &Ty<DefinitionId>, right: &Ty<DefinitionId>| {
            left.canonical_cmp(right, table, cancel)
                .is_ok_and(|order| order.is_lt())
        };
        if !bounds_with(&function.bounds, &valid, &before)
            || declaration
                .concrete_result
                .as_ref()
                .is_some_and(|ty| !matches!(function.return_type, Ty::Trait(_)) || !valid(ty))
            || !function
                .params
                .iter()
                .map(|param| &param.ty)
                .chain(iter::once(&function.return_type))
                .all(&valid)
            || declaration.callable_requirements.len() > 4096
        {
            return Err(DeclarationValidationError::Invalid);
        }
        for required in &declaration.callable_requirements {
            if table.parent(required.member) != Ok(Some(required.interface.declaration))
                || !table
                    .resolve(required.member)
                    .ok()
                    .and_then(|view| view.last())
                    .is_some_and(|part| {
                        part.kind == DefinitionKind::Method
                            && part.occurrence == 0
                            && !part.name.is_empty()
                    })
                || required.arguments.len() > 4096
                || !valid(&required.receiver)
                || !valid(&Ty::Trait(required.interface.clone()))
                || required.arguments.iter().any(|ty| !valid(ty))
            {
                return Err(DeclarationValidationError::Invalid);
            }
        }
    }
    Ok(())
}

fn bounds_with<I: DefinitionReference>(
    bounds: &[GenericBound<I>],
    valid: &impl Fn(&Ty<I>) -> bool,
    before: &impl Fn(&Ty<I>, &Ty<I>) -> bool,
) -> bool {
    bounds
        .windows(2)
        .all(|pair| before(&pair[0].ty, &pair[1].ty))
        && bounds.iter().all(|bound| {
            valid(&bound.ty)
                && !bound.constraints.is_empty()
                && constraints_with(&bound.constraints, valid, before)
        })
}

fn constraints_with<I: DefinitionReference>(
    constraints: &[Constraint<I>],
    valid: &impl Fn(&Ty<I>) -> bool,
    before: &impl Fn(&Ty<I>, &Ty<I>) -> bool,
) -> bool {
    constraints
        .windows(2)
        .all(|pair| match (&pair[0], &pair[1]) {
            (Constraint::Standard(left), Constraint::Standard(right)) => left < right,
            (Constraint::Standard(_), Constraint::Trait(_)) => true,
            (Constraint::Trait(_), Constraint::Standard(_)) => false,
            (Constraint::Trait(left), Constraint::Trait(right)) => {
                before(&Ty::Trait(left.clone()), &Ty::Trait(right.clone()))
            }
        })
        && constraints.iter().all(|constraint| match constraint {
            Constraint::Standard(_) => true,
            Constraint::Trait(ty) => valid(&Ty::Trait(ty.clone())),
        })
}

pub fn trait_valid(
    ty: &TraitDef<DefinitionId>,
    owner: DefinitionId,
    cancel: &CancellationToken,
    table: &DefinitionTable,
) -> bool {
    if !table.resolve(owner).is_ok_and(|view| {
        view.segments().count() == 1
            && view.last().is_some_and(|part| {
                part.kind == DefinitionKind::Trait && part.name == ty.name && part.occurrence == 0
            })
    }) {
        return false;
    }
    let member_of = |id, kind| {
        table.parent(id) == Ok(Some(owner))
            && table
                .resolve(id)
                .ok()
                .and_then(|view| view.last())
                .is_some_and(|part| {
                    part.kind == kind && part.occurrence == 0 && !part.name.is_empty()
                })
    };
    let mut methods = HashSet::new();
    !ty.name.is_empty()
        && ty
            .conversion_adapter
            .as_ref()
            .is_none_or(|adapter| adapter.valid_in(&owner, ty, table))
        && {
            let mut members = HashSet::new();
            ty.associated_consts.iter().all(|member| {
                members.insert(member.declaration)
                    && member_of(member.declaration, DefinitionKind::Const)
                    && scalar_const_type(&member.ty)
                    && member
                        .default_value
                        .as_ref()
                        .is_none_or(|value| scalar_const_valid(&member.ty, value))
            })
        }
        && {
            let mut members = HashSet::new();
            ty.associated_types.iter().all(|member| {
                members.insert(member.declaration)
                    && member_of(member.declaration, DefinitionKind::AssociatedType)
            })
        }
        && parameters(&ty.generic_params, &owner, &Parameters::new()).is_some_and(|params| {
            bounds_valid(&ty.bounds, &params, cancel, table)
                && ty.supertraits.iter().all(|parent| {
                    type_valid(
                        &Ty::Trait(parent.clone()),
                        &params,
                        Some(&owner),
                        cancel,
                        table,
                    )
                })
                && ty.associated_types.iter().all(|member| {
                    parameters(&member.generic_params, &member.declaration, &params).is_some_and(
                        |params| {
                            bounds_valid_in(
                                &member.parameter_bounds,
                                &params,
                                Some(&owner),
                                cancel,
                                table,
                            ) && constraints_valid(
                                &member.bounds,
                                &params,
                                Some(&owner),
                                cancel,
                                table,
                            )
                        },
                    )
                })
                && ty.methods.iter().all(|method| {
                    methods.insert(&method.name)
                        && (method.method_policy.override_allowed
                            || !matches!(method.implementation, CallableImplementation::Required))
                        && table
                            .lookup_child(owner, DefinitionKind::Method, &method.name, 0)
                            .is_some_and(|id| {
                                function_valid(method, id, &params, Some(&owner), cancel, table)
                            })
                })
        })
}

pub fn aggregate_shape_valid<I: DefinitionReference>(
    ty: &TypeDef<I>,
    cancel: &CancellationToken,
) -> bool {
    if ty.name.is_empty()
        || match ty.kind {
            TypeDefKind::Struct => !ty.variants.is_empty(),
            TypeDefKind::Enum => !ty.fields.is_empty(),
            TypeDefKind::Native(kind) => !kind.shape_valid(ty),
            TypeDefKind::NativeStorage(layout) => {
                !ty.fields.is_empty()
                    || !ty.variants.is_empty()
                    || !layout.valid_parameters(ty.generic_params.len())
            }
        }
    {
        return false;
    }
    let mut names = HashSet::new();
    ty.fields
        .iter()
        .map(|field| &field.name)
        .chain(ty.variants.iter().map(|variant| &variant.name))
        .all(|name| cancel.check().is_ok() && !name.is_empty() && names.insert(name))
}

pub fn parameters(
    declared: &[GenericParam<DefinitionId>],
    owner: &DefinitionId,
    outer: &Parameters,
) -> Option<Parameters> {
    let mut params = outer.clone();
    for (position, param) in declared.iter().enumerate() {
        if &param.owner != owner || param.position != position || !params.insert((*owner, position))
        {
            return None;
        }
    }
    Some(params)
}

pub fn bounds_valid(
    bounds: &[GenericBound<DefinitionId>],
    params: &Parameters,
    cancel: &CancellationToken,
    table: &DefinitionTable,
) -> bool {
    bounds_valid_in(bounds, params, None, cancel, table)
}

fn bounds_valid_in(
    bounds: &[GenericBound<DefinitionId>],
    params: &Parameters,
    self_owner: Option<&DefinitionId>,
    cancel: &CancellationToken,
    table: &DefinitionTable,
) -> bool {
    bounds_with(
        bounds,
        &|ty| type_valid(ty, params, self_owner, cancel, table),
        &|left, right| {
            left.canonical_cmp(right, table, cancel)
                .is_ok_and(|order| order.is_lt())
        },
    )
}

pub fn native_bounds_valid(
    bounds: &[GenericBound<DefinitionId>],
    parameters: &[GenericParam<DefinitionId>],
    cancel: &CancellationToken,
    table: &DefinitionTable,
) -> bool {
    let params = parameters
        .iter()
        .map(|param| (param.owner, param.position))
        .collect();
    bounds.len() <= 4096
        && parameters.len() <= 4096
        && bounds.iter().all(|bound| bound.constraints.len() <= 4096)
        && bounds_with(
            bounds,
            &|ty| type_valid_in(ty, &params, None, cancel, Some(table)),
            &|left, right| {
                left.canonical_cmp(right, table, cancel)
                    .is_ok_and(|order| order.is_lt())
            },
        )
}

fn constraints_valid(
    constraints: &[Constraint<DefinitionId>],
    params: &Parameters,
    self_owner: Option<&DefinitionId>,
    cancel: &CancellationToken,
    table: &DefinitionTable,
) -> bool {
    constraints_with(
        constraints,
        &|ty| type_valid(ty, params, self_owner, cancel, table),
        &|left, right| {
            left.canonical_cmp(right, table, cancel)
                .is_ok_and(|order| order.is_lt())
        },
    )
}

pub fn function_valid(
    function: &FnDecl<DefinitionId>,
    owner: DefinitionId,
    outer: &Parameters,
    self_owner: Option<&DefinitionId>,
    cancel: &CancellationToken,
    table: &DefinitionTable,
) -> bool {
    let Ok(view) = table.resolve(owner) else {
        return false;
    };
    let Ok(parent) = table.parent(owner) else {
        return false;
    };
    let parent_kind =
        parent.and_then(|parent| table.resolve(parent).ok()?.last().map(|part| part.kind));
    let kind = if parent_kind.is_none() {
        DefinitionKind::Function
    } else {
        DefinitionKind::Method
    };
    if !view
        .last()
        .is_some_and(|part| part.kind == kind && part.name == function.name && part.occurrence == 0)
    {
        return false;
    }
    let callable = |id: &DefinitionId| {
        id.describe(Some(table)).is_ok_and(|view| {
            view.nominal(DefinitionKind::Function) || view.nominal(DefinitionKind::Method)
        })
    };
    let implementation_valid = match &function.implementation {
        CallableImplementation::Required => parent_kind == Some(DefinitionKind::Trait),
        CallableImplementation::Script => true,
        CallableImplementation::Native(id) => callable(id),
        CallableImplementation::NativeDefault(application) => {
            matches!(
                parent_kind,
                Some(DefinitionKind::Trait | DefinitionKind::Impl)
            ) && callable(&application.declaration)
                && application.arguments.len() <= 4096
        }
    };
    if function.name.is_empty()
        || !implementation_valid
        || (!function.method_policy.override_allowed
            && !matches!(
                function.implementation,
                CallableImplementation::NativeDefault(_)
            )
            && parent_kind != Some(DefinitionKind::Trait))
    {
        return false;
    }
    parameters(&function.generic_params, &owner, outer).is_some_and(|params| {
        bounds_valid_in(&function.bounds, &params, self_owner, cancel, table)
            && function
                .params
                .iter()
                .map(|param| &param.ty)
                .chain(iter::once(&function.return_type))
                .all(|ty| type_valid(ty, &params, self_owner, cancel, table))
            && match &function.implementation {
                CallableImplementation::NativeDefault(application) => application
                    .arguments
                    .iter()
                    .all(|argument| type_valid(argument, &params, self_owner, cancel, table)),
                _ => true,
            }
    })
}

/// Check portable type expressions against an explicitly supplied binder scope.
pub fn types_in_scope<'a>(
    types: impl IntoIterator<Item = &'a Ty>,
    parameters: &[GenericParam],
    cancel: &CancellationToken,
) -> bool {
    let parameters = parameters
        .iter()
        .map(|parameter| (parameter.owner.clone(), parameter.position))
        .collect();
    types
        .into_iter()
        .all(|ty| ty.within_wire_limits() && type_valid_in(ty, &parameters, None, cancel, None))
}

pub fn type_valid(
    ty: &Ty<DefinitionId>,
    params: &Parameters,
    self_owner: Option<&DefinitionId>,
    cancel: &CancellationToken,
    table: &DefinitionTable,
) -> bool {
    type_valid_in(ty, params, self_owner, cancel, Some(table))
}

/// Check scoped type expressions without materializing owned paths.
pub fn types_in_scope_in<'a, I: DefinitionReference + 'a>(
    types: impl IntoIterator<Item = &'a Ty<I>>,
    parameters: &[GenericParam<I>],
    cancel: &CancellationToken,
    table: Option<&DefinitionTable>,
) -> bool {
    if parameters
        .iter()
        .any(|parameter| parameter.owner.describe(table).is_err())
    {
        return false;
    }
    let parameters = parameters
        .iter()
        .map(|parameter| (parameter.owner.clone(), parameter.position))
        .collect();
    types
        .into_iter()
        .all(|ty| ty.within_wire_limits() && type_valid_in(ty, &parameters, None, cancel, table))
}

fn type_valid_in<I: DefinitionReference>(
    ty: &Ty<I>,
    params: &HashSet<(I, usize)>,
    self_owner: Option<&I>,
    cancel: &CancellationToken,
    table: Option<&DefinitionTable>,
) -> bool {
    let associated = |member: &I, parent: &I| match (member.describe(table), parent.describe(table))
    {
        (Ok(member), Ok(parent)) => member.associated_member(parent),
        _ => false,
    };
    let nominal_valid = |id: &I, kind| id.describe(table).is_ok_and(|id| id.nominal(kind));
    let mut pending = vec![ty];
    while let Some(ty) = pending.pop() {
        if cancel.check().is_err() {
            return false;
        }
        match ty {
            Ty::Projection {
                receiver,
                interface,
                member,
                arguments,
            } => {
                if params.is_empty() && self_owner.is_none() {
                    return false;
                }
                if !associated(member, &interface.declaration)
                    || !nominal_valid(&interface.declaration, DefinitionKind::Trait)
                {
                    return false;
                }
                pending.push(receiver);
                pending.extend(arguments);
                for (binding, value) in &interface.associated_types {
                    if !associated(binding, &interface.declaration) {
                        return false;
                    }
                    pending.push(value);
                }
                pending.extend(&interface.arguments);
            }
            Ty::Parameter { owner, position } => {
                if owner.describe(table).is_err() || !params.contains(&(owner.clone(), *position)) {
                    return false;
                }
            }
            Ty::SelfType(owner) => {
                if owner.describe(table).is_err() || Some(owner) != self_owner {
                    return false;
                }
            }
            Ty::Builtin(_) => {}
            Ty::Host(id) => {
                if validate_host_type_identity_in(id, table).is_err() {
                    return false;
                }
            }
            Ty::Tuple(types) => pending.extend(types),
            Ty::Function { params, result } => {
                pending.extend(params);
                pending.push(result);
            }
            Ty::Range(ty, kind) => {
                if *kind == RangeKind::Full {
                    if **ty != Ty::Builtin(BuiltinType::Unit) {
                        return false;
                    }
                } else {
                    match ty.as_ref() {
                        Ty::Builtin(t) if t.integer_layout().is_some() => {}
                        Ty::Parameter { .. } | Ty::Projection { .. } | Ty::SelfType(_) => {
                            pending.push(ty)
                        }
                        _ => return false,
                    }
                }
            }
            Ty::Array(ty) | Ty::Set(ty, _) | Ty::Iter(ty) => pending.push(ty),
            Ty::Map { key, value, .. } => pending.extend([key.as_ref(), value.as_ref()]),

            Ty::Struct(nominal)
            | Ty::NativeObject(nominal)
            | Ty::Enum(nominal)
            | Ty::Trait(nominal) => {
                pending.extend(&nominal.arguments);
                if !matches!(
                    nominal
                        .declaration
                        .describe(table)
                        .ok()
                        .and_then(|view| view.last())
                        .map(|part| part.kind),
                    Some(DefinitionKind::Trait)
                ) && !nominal.associated_types.is_empty()
                {
                    return false;
                }
                for (member, value) in &nominal.associated_types {
                    if !associated(member, &nominal.declaration) {
                        return false;
                    }
                    pending.push(value);
                }
            }
        }
        let nominal = match ty {
            Ty::Struct(n) => Some((n, DefinitionKind::Struct)),
            Ty::NativeObject(n) => Some((n, DefinitionKind::AssociatedType)),
            Ty::Enum(n) => Some((n, DefinitionKind::Enum)),
            Ty::Trait(n) => Some((n, DefinitionKind::Trait)),
            _ => None,
        };
        if let Some((nominal, kind)) = nominal
            && !nominal_valid(&nominal.declaration, kind)
        {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod scoped_tests;

/// Check fields, variants, storage parameters and binder ownership together.
pub fn type_definition_valid(
    ty: &TypeDef<DefinitionId>,
    owner: DefinitionId,
    cancel: &CancellationToken,
    table: &DefinitionTable,
) -> bool {
    let kind = ty.kind.definition_kind();
    if !table.resolve(owner).is_ok_and(|view| {
        view.segments().count() == 1
            && view.last().is_some_and(|part| {
                part.kind == kind && part.name == ty.name && part.occurrence == 0
            })
    }) {
        return false;
    }
    aggregate_shape_valid(ty, cancel)
        && ty
            .variants
            .iter()
            .all(|variant| !variant.reports_failure || variant.payload.len() == 1)
        && parameters(&ty.generic_params, &owner, &Parameters::new()).is_some_and(|params| {
            bounds_valid(&ty.bounds, &params, cancel, table)
                && ty
                    .fields
                    .iter()
                    .all(|field| type_valid(&field.ty, &params, None, cancel, table))
                && ty
                    .variants
                    .iter()
                    .flat_map(|variant| &variant.payload)
                    .all(|ty| type_valid(ty, &params, None, cancel, table))
        })
}

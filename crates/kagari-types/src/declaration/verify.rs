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
    identity,
    identity::{
        DefinitionKind, DefinitionPath, DefinitionPathSegment, ModuleIdentity,
        reference::DefinitionReference, table::DefinitionTable,
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
pub type Parameters = HashSet<(DefinitionPath, usize)>;

fn scalar_const_type(ty: &Ty) -> bool {
    matches!(
        ty,
        Ty::Builtin(BuiltinType::Unit | BuiltinType::Bool | BuiltinType::I32 | BuiltinType::F32)
    )
}

pub fn scalar_const_valid(ty: &Ty, value: &str) -> bool {
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

pub fn concrete_type_valid(ty: &Ty, cancel: &CancellationToken) -> bool {
    type_valid(ty, &Parameters::new(), None, cancel)
}

/// Validate declaration binders before any concrete native import can use them.
/// The import separately matches this template and the engine's storage guard.
pub fn validate_native_declarations(
    declarations: &[NativeDeclaration],
    module: &ModuleIdentity,
    cancel: &CancellationToken,
) -> Result<(), DeclarationValidationError> {
    let mut identities = HashSet::new();
    for declaration in declarations {
        cancel
            .check()
            .map_err(|_| DeclarationValidationError::Cancelled)?;
        let id = &declaration.declaration;
        let function = &declaration.function;
        if id.module != *module
            || !id.within_path_limit()
            || !identities.insert(id)
            || !id.path.last().is_some_and(|part| {
                matches!(part.kind, DefinitionKind::Function | DefinitionKind::Method)
                    && part.name == function.name
            })
            || !matches!(
                &function.implementation,
                CallableImplementation::Native(binding)
                    if binding.within_path_limit()
                        && (nominal_valid(binding, DefinitionKind::Function)
                            || nominal_valid(binding, DefinitionKind::Method))
            )
        {
            return Err(DeclarationValidationError::Invalid);
        }
        let mut owners = BTreeMap::<_, Vec<_>>::new();
        let mut params = Parameters::new();
        for parameter in &function.generic_params {
            if parameter.owner.module != *module
                || !id.path.starts_with(&parameter.owner.path)
                || !params.insert((parameter.owner.clone(), parameter.position))
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
        let mut receiver = id.clone();
        receiver.path.pop();
        let self_owner = receiver
            .path
            .last()
            .is_some_and(|part| part.kind == DefinitionKind::Trait)
            .then_some(&receiver);
        if !bounds_valid_in(&function.bounds, &params, self_owner, cancel)
            || declaration.concrete_result.as_ref().is_some_and(|ty| {
                !matches!(function.return_type, Ty::Trait(_))
                    || !type_valid(ty, &params, self_owner, cancel)
            })
            || !signature_valid(function, &params, self_owner, cancel)
            || declaration.callable_requirements.len() > 4096
        {
            return Err(DeclarationValidationError::Invalid);
        }
        for required in &declaration.callable_requirements {
            let mut owner = required.member.clone();
            let member = owner.path.pop();
            if owner != required.interface.declaration
                || !member.is_some_and(|part| {
                    part.kind == DefinitionKind::Method
                        && part.occurrence == 0
                        && !part.name.is_empty()
                })
                || !required.member.within_path_limit()
                || required.arguments.len() > 4096
                || !type_valid(&required.receiver, &params, self_owner, cancel)
                || !type_valid(
                    &Ty::Trait(required.interface.clone()),
                    &params,
                    self_owner,
                    cancel,
                )
                || required
                    .arguments
                    .iter()
                    .any(|ty| !type_valid(ty, &params, self_owner, cancel))
            {
                return Err(DeclarationValidationError::Invalid);
            }
        }
    }
    Ok(())
}

pub fn trait_valid(ty: &TraitDef, module: &ModuleIdentity, cancel: &CancellationToken) -> bool {
    let owner = owner(module, &[], DefinitionKind::Trait, &ty.name);
    let mut methods = HashSet::new();
    !ty.name.is_empty()
        && ty
            .conversion_adapter
            .as_ref()
            .is_none_or(|adapter| adapter.valid_in(&owner, ty))
        && {
            let mut members = HashSet::new();
            ty.associated_consts.iter().all(|member| {
                let name = member
                    .declaration
                    .path
                    .last()
                    .map_or("", |part| part.name.as_str());
                !name.is_empty()
                    && members.insert(&member.declaration)
                    && member.declaration == identity::associated_const_id(&owner, name)
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
                members.insert(&member.declaration)
                    && member.declaration
                        == identity::associated_type_id(
                            &owner,
                            member
                                .declaration
                                .path
                                .last()
                                .map_or("", |part| part.name.as_str()),
                        )
                    && member
                        .declaration
                        .path
                        .last()
                        .is_some_and(|part| !part.name.is_empty())
            })
        }
        && parameters(&ty.generic_params, &owner, &Parameters::new()).is_some_and(|params| {
            bounds_valid(&ty.bounds, &params, cancel)
                && ty.supertraits.iter().all(|parent| {
                    type_valid(&Ty::Trait(parent.clone()), &params, Some(&owner), cancel)
                })
                && ty.associated_types.iter().all(|member| {
                    parameters(&member.generic_params, &member.declaration, &params).is_some_and(
                        |params| {
                            bounds_valid_in(&member.parameter_bounds, &params, Some(&owner), cancel)
                                && constraints_valid(&member.bounds, &params, Some(&owner), cancel)
                        },
                    )
                })
                && ty.methods.iter().all(|method| {
                    methods.insert(&method.name)
                        && (method.method_policy.override_allowed
                            || !matches!(method.implementation, CallableImplementation::Required))
                        && function_valid(
                            method,
                            module,
                            &owner.path,
                            &params,
                            Some(&owner),
                            cancel,
                        )
                })
        })
}

pub fn aggregate_shape_valid(ty: &TypeDef, cancel: &CancellationToken) -> bool {
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

pub fn owner(
    module: &ModuleIdentity,
    parent: &[DefinitionPathSegment],
    kind: DefinitionKind,
    name: &str,
) -> DefinitionPath {
    let mut path = parent.to_vec();
    path.push(DefinitionPathSegment {
        kind,
        name: name.into(),
        occurrence: 0,
    });
    DefinitionPath {
        module: module.clone(),
        path,
    }
}

pub fn parameters(
    declared: &[GenericParam],
    owner: &DefinitionPath,
    outer: &Parameters,
) -> Option<Parameters> {
    let mut params = outer.clone();
    for (position, param) in declared.iter().enumerate() {
        if &param.owner != owner
            || param.position != position
            || !params.insert((owner.clone(), position))
        {
            return None;
        }
    }
    Some(params)
}

pub fn bounds_valid(
    bounds: &[GenericBound],
    params: &Parameters,
    cancel: &CancellationToken,
) -> bool {
    bounds_valid_in(bounds, params, None, cancel)
}

fn bounds_valid_in(
    bounds: &[GenericBound],
    params: &Parameters,
    self_owner: Option<&DefinitionPath>,
    cancel: &CancellationToken,
) -> bool {
    if !bounds.windows(2).all(|pair| pair[0].ty < pair[1].ty) {
        return false;
    }
    let mut seen = HashSet::new();
    bounds.iter().all(|bound| {
        type_valid(&bound.ty, params, self_owner, cancel)
            && seen.insert(&bound.ty)
            && !bound.constraints.is_empty()
            && constraints_valid(&bound.constraints, params, self_owner, cancel)
    })
}

pub fn native_bounds_valid(
    bounds: &[GenericBound],
    parameters: &[GenericParam],
    cancel: &CancellationToken,
) -> bool {
    let params = parameters
        .iter()
        .map(|param| (param.owner.clone(), param.position))
        .collect();
    bounds.len() <= 4096
        && parameters.len() <= 4096
        && bounds.iter().all(|bound| bound.constraints.len() <= 4096)
        && bounds_valid(bounds, &params, cancel)
}

fn constraints_valid(
    constraints: &[Constraint],
    params: &Parameters,
    self_owner: Option<&DefinitionPath>,
    cancel: &CancellationToken,
) -> bool {
    constraints.windows(2).all(|pair| pair[0] < pair[1])
        && constraints.iter().all(|constraint| match constraint {
            Constraint::Standard(_) => true,
            Constraint::Trait(ty) => type_valid(&Ty::Trait(ty.clone()), params, self_owner, cancel),
        })
}

pub fn function_valid(
    function: &FnDecl,
    module: &ModuleIdentity,
    parent: &[DefinitionPathSegment],
    outer: &Parameters,
    self_owner: Option<&DefinitionPath>,
    cancel: &CancellationToken,
) -> bool {
    let implementation_valid = match &function.implementation {
        CallableImplementation::Required => parent
            .last()
            .is_some_and(|owner| owner.kind == DefinitionKind::Trait),
        CallableImplementation::Script => true,
        // Provider authentication and signature matching are linked-program checks.
        CallableImplementation::Native(id) => {
            (nominal_valid(id, DefinitionKind::Function)
                || nominal_valid(id, DefinitionKind::Method))
                && id.within_path_limit()
        }
        CallableImplementation::NativeDefault(application) => {
            parent.last().is_some_and(|owner| {
                matches!(owner.kind, DefinitionKind::Trait | DefinitionKind::Impl)
            }) && application.declaration.within_path_limit()
                && (nominal_valid(&application.declaration, DefinitionKind::Function)
                    || nominal_valid(&application.declaration, DefinitionKind::Method))
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
            && !parent
                .last()
                .is_some_and(|owner| owner.kind == DefinitionKind::Trait))
    {
        return false;
    }
    let kind = if parent.is_empty() {
        DefinitionKind::Function
    } else {
        DefinitionKind::Method
    };
    let owner = owner(module, parent, kind, &function.name);
    parameters(&function.generic_params, &owner, outer).is_some_and(|params| {
        bounds_valid_in(&function.bounds, &params, self_owner, cancel)
            && signature_valid(function, &params, self_owner, cancel)
            && match &function.implementation {
                CallableImplementation::NativeDefault(application) => application
                    .arguments
                    .iter()
                    .all(|argument| type_valid(argument, &params, self_owner, cancel)),
                _ => true,
            }
    })
}

fn signature_valid(
    function: &FnDecl,
    params: &Parameters,
    self_owner: Option<&DefinitionPath>,
    cancel: &CancellationToken,
) -> bool {
    function
        .params
        .iter()
        .map(|param| &param.ty)
        .chain(iter::once(&function.return_type))
        .all(|ty| type_valid(ty, params, self_owner, cancel))
}

fn nominal_valid(id: &DefinitionPath, kind: DefinitionKind) -> bool {
    !id.module.package.0.is_empty()
        && !id.module.path.is_empty()
        && !id.module.path.iter().any(String::is_empty)
        && id
            .path
            .last()
            .is_some_and(|part| part.kind == kind && !part.name.is_empty())
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
        .all(|ty| ty.within_wire_limits() && type_valid(ty, &parameters, None, cancel))
}

pub fn type_valid(
    ty: &Ty,
    params: &Parameters,
    self_owner: Option<&DefinitionPath>,
    cancel: &CancellationToken,
) -> bool {
    type_valid_in(ty, params, self_owner, cancel, None)
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
                if !params.contains(&(owner.clone(), *position)) {
                    return false;
                }
            }
            Ty::SelfType(owner) => {
                if Some(owner) != self_owner {
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
    ty: &TypeDef,
    module: &ModuleIdentity,
    cancel: &CancellationToken,
) -> bool {
    let kind = ty.kind.definition_kind();
    let owner = owner(module, &[], kind, &ty.name);
    aggregate_shape_valid(ty, cancel)
        && ty
            .variants
            .iter()
            .all(|variant| !variant.reports_failure || variant.payload.len() == 1)
        && parameters(&ty.generic_params, &owner, &Parameters::new()).is_some_and(|params| {
            bounds_valid(&ty.bounds, &params, cancel)
                && ty
                    .fields
                    .iter()
                    .all(|field| type_valid(&field.ty, &params, None, cancel))
                && ty
                    .variants
                    .iter()
                    .flat_map(|variant| &variant.payload)
                    .all(|ty| type_valid(ty, &params, None, cancel))
        })
}

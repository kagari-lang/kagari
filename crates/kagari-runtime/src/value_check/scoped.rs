//! Complete value matching reads actual providers and preserves lexical type views.
use crate::{
    Runtime,
    error::{RuntimeError, RuntimeErrorKind},
    frame::{
        arguments::ArgumentError,
        types::{arguments::TypeArgument, compatibility::TypeView},
    },
    host::{HostBorrowKind, HostBorrowTable, HostRegistry},
    metadata::TypeId,
    module::ExecutableModule,
    range::RangeValue,
    resource::ResourceState,
    value::{
        EphemeralValue,
        aggregates::{enumerations::EnumObject, structures::StructObject, tuples::TupleObject},
        arrays::ArrayObject,
        carriers::ScopedValue,
        closures::ClosureObject,
        host_values::{EphemeralObject, HostRootObject, paths::HostPathObject},
        interfaces::InterfaceObject,
        kind::ObjectKind,
        native::NativeObject,
        scalar,
    },
};
use kagari_contract::representation::semantic_representation;
use kagari_gc::{
    access::Scope,
    error::GcError,
    object::{Gc, GcObject},
};
use kagari_types::ty::Ty;
use std::collections::TryReserveError;

#[derive(Debug)]
pub(crate) enum CheckError {
    Storage(GcError),
    Execution(RuntimeError),
}

impl From<ArgumentError> for CheckError {
    fn from(error: ArgumentError) -> Self {
        match error {
            ArgumentError::Input(error) => Self::Execution(error),
            ArgumentError::Storage(error) => Self::Storage(error),
        }
    }
}

impl From<GcError> for CheckError {
    fn from(error: GcError) -> Self {
        Self::Storage(error)
    }
}

impl From<RuntimeError> for CheckError {
    fn from(error: RuntimeError) -> Self {
        Self::Execution(error)
    }
}

impl From<TryReserveError> for CheckError {
    fn from(_: TryReserveError) -> Self {
        Self::Storage(GcError::Allocation)
    }
}

impl CheckError {
    pub(crate) fn into_runtime(self, runtime: &Runtime) -> RuntimeError {
        match self {
            Self::Storage(error) => runtime.managed_memory_error(error),
            Self::Execution(error) => error,
        }
    }
}

struct TupleMembers<'gc, 'ty> {
    object: Gc<'gc, TupleObject>,
    expected: TypeView<'ty>,
    next: usize,
    count: usize,
}

enum Match<'gc, 'ty> {
    Leaf(bool),
    Tuple(TupleMembers<'gc, 'ty>),
}

/// Prepared nominal facts retain their admitted identity optimization. Other
/// types use the same complete lexical matcher, without an extra source/root pack.
pub(crate) fn matches_argument<'gc>(
    cx: &Scope<'gc>,
    value: ScopedValue<'gc>,
    expected: &TypeArgument,
    owner: &ExecutableModule,
    host: &HostRegistry,
    borrows: &HostBorrowTable,
    resources: &ResourceState,
) -> Result<bool, CheckError> {
    match expected.matches_admitted_scoped(cx, value, owner, resources)? {
        Some(matched) => Ok(matched),
        None => matches(cx, value, expected.view(owner), host, borrows, resources),
    }
}

/// Scalar and flat tuple checks allocate no traversal stack. Only unfinished
/// outer tuple siblings occupy scratch. No value/source loan crosses execution.
pub(crate) fn matches<'gc>(
    cx: &Scope<'gc>,
    value: ScopedValue<'gc>,
    expected: TypeView<'_>,
    host: &HostRegistry,
    borrows: &HostBorrowTable,
    resources: &ResourceState,
) -> Result<bool, CheckError> {
    let mut current = Some((value, expected));
    let mut active: Option<TupleMembers<'_, '_>> = None;
    let mut pending = Vec::new();
    let mut work = 0usize;
    loop {
        if let Some((value, expected)) = current.take() {
            if work.is_multiple_of(1024) {
                resources.poll_closed()?;
            }
            work += 1;
            match single(cx, value, expected, host, borrows, resources)? {
                Match::Leaf(false) => return Ok(false),
                Match::Leaf(true) => {}
                Match::Tuple(child) => {
                    if let Some(parent) = active.take()
                        && parent.next < parent.count
                    {
                        pending.try_reserve(1)?;
                        pending.push(parent);
                    }
                    active = Some(child);
                }
            }
        }
        if let Some(tuple) = active.as_mut() {
            if tuple.next < tuple.count {
                let expected = tuple
                    .expected
                    .parameter(tuple.next)
                    .ok_or(GcError::Representation)?;
                let value = TupleObject::get(cx, tuple.object, tuple.next)?;
                tuple.next += 1;
                current = Some((value, expected));
            } else {
                active = pending.pop();
            }
        } else {
            return Ok(true);
        }
    }
}

fn object<'gc, T: GcObject>(
    cx: &Scope<'gc>,
    value: ScopedValue<'gc>,
) -> Result<Gc<'gc, T>, GcError> {
    cx.downcast(value.object().ok_or(GcError::Representation)?)
}

fn single<'gc, 'ty>(
    cx: &Scope<'gc>,
    value: ScopedValue<'gc>,
    expected: TypeView<'ty>,
    host: &HostRegistry,
    borrows: &HostBorrowTable,
    resources: &ResourceState,
) -> Result<Match<'gc, 'ty>, CheckError> {
    let Some(expected) = expected.normalized() else {
        return Ok(Match::Leaf(false));
    };
    let result = match expected.ty {
        Ty::Builtin(kind) if kind.integer_layout().is_some() => {
            scalar::read_integer(*kind, value).is_ok()
        }
        Ty::Builtin(_) => value.has_representation(semantic_representation(expected.ty)),
        Ty::Tuple(types) if value.kind() == Some(ObjectKind::Tuple) => {
            let object = object::<TupleObject>(cx, value)?;
            if cx.read(object, TupleObject::len) != types.len() {
                false
            } else if types.is_empty() {
                true
            } else {
                return Ok(Match::Tuple(TupleMembers {
                    object,
                    expected,
                    next: 0,
                    count: types.len(),
                }));
            }
        }
        Ty::Range(_, _) if value.kind() == Some(ObjectKind::Range) => {
            let object = object::<RangeValue>(cx, value)?;
            expected
                .closed()
                .is_some_and(|ty| cx.read(object, |object| object.matches(&ty)))
        }
        Ty::Array(_) if value.kind() == Some(ObjectKind::Array) => {
            let object = object::<ArrayObject>(cx, value)?;
            expected.parameter(0).is_some_and(|element| {
                cx.read(object, |object| object.contract().matches_view(element))
            })
        }
        Ty::Struct(nominal) if value.kind() == Some(ObjectKind::Struct) => {
            let object = object::<StructObject>(cx, value)?;
            cx.read(object, |object| {
                if expected.environment.is_none() {
                    expected
                        .owner
                        .find_struct_layout(nominal)
                        .is_some_and(|layout| object.layout().matches(&layout))
                } else {
                    object.layout().matches_view(expected)
                }
            })
        }
        Ty::Enum(_) if value.kind() == Some(ObjectKind::Enum) => {
            let object = object::<EnumObject>(cx, value)?;
            cx.read(object, |object| object.layout().matches_view(expected))
        }
        Ty::NativeObject(_) if value.kind() == Some(ObjectKind::Native) => {
            let object = object::<NativeObject>(cx, value)?;
            cx.read(object, |object| object.matches(expected))
        }
        Ty::Trait(_) if value.kind() == Some(ObjectKind::Interface) => {
            let object = object::<InterfaceObject>(cx, value)?;
            cx.read(object, |object| {
                object.description().interface_view().compatible(expected)
            })
        }
        Ty::Function { .. } if value.kind() == Some(ObjectKind::Closure) => {
            let object = object::<ClosureObject>(cx, value)?;
            cx.read(object, |object| object.record().matches_view(expected))
        }
        Ty::Host(id) => match expected.owner.host_type(*id) {
            Some(ty) => host_matches(cx, value, ty, host, borrows, resources)?,
            None => false,
        },
        Ty::Reference { access, .. }
            if value.kind() == Some(ObjectKind::Ephemeral)
                && value.as_runtime_ephemeral().is_none() =>
        {
            let object = object::<EphemeralObject>(cx, value)?;
            match cx.read(object, EphemeralObject::value) {
                EphemeralValue::HostView(view) => expected
                    .reference_target()
                    .is_some_and(|target| borrows.scoped_matches_view(view, target, *access)),
                _ => false,
            }
        }
        Ty::Projection { .. } | Ty::SelfType(_) => match expected.closed() {
            Some(closed) if closed.is_concrete() => matches(
                cx,
                value,
                expected.resolved(&closed),
                host,
                borrows,
                resources,
            )?,
            _ => false,
        },
        _ => false,
    };
    Ok(Match::Leaf(result))
}

fn host_matches<'gc>(
    cx: &Scope<'gc>,
    value: ScopedValue<'gc>,
    expected: TypeId,
    host: &HostRegistry,
    borrows: &HostBorrowTable,
    resources: &ResourceState,
) -> Result<bool, CheckError> {
    Ok(match value.kind() {
        Some(ObjectKind::HostRoot) => {
            let object = object::<HostRootObject>(cx, value)?;
            let root = cx.read(object, HostRootObject::handle);
            root.type_id() == expected && host.matches_root(root)
        }
        Some(ObjectKind::HostPath) => {
            let object = object::<HostPathObject>(cx, value)?;
            if cx.read(object, |object| object.facts().result) != expected {
                return Ok(false);
            }
            valid_capability(host.validate_path_scoped(cx, object, resources)?)?
        }
        Some(ObjectKind::Ephemeral) if value.as_runtime_ephemeral().is_none() => {
            let object = object::<EphemeralObject>(cx, value)?;
            match cx.read(object, EphemeralObject::value) {
                EphemeralValue::HostRef(token) => {
                    token.type_id() == expected
                        && valid_capability(borrows.validate(token, HostBorrowKind::Shared))?
                }
                EphemeralValue::HostMut(token) => {
                    token.type_id() == expected
                        && valid_capability(borrows.validate(token, HostBorrowKind::Unique))?
                }
                _ => false,
            }
        }
        _ => false,
    })
}

fn valid_capability(result: Result<(), RuntimeError>) -> Result<bool, CheckError> {
    match result {
        Ok(()) => Ok(true),
        Err(error)
            if matches!(
                error.kind(),
                RuntimeErrorKind::Cancelled
                    | RuntimeErrorKind::EngineFault
                    | RuntimeErrorKind::ResourceLimitExceeded
            ) =>
        {
            Err(error.into())
        }
        Err(_) => Ok(false),
    }
}

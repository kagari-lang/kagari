//! Closed value comparisons borrow real providers, without object snapshots.
use crate::{
    error::{RuntimeError, trap::TrapReason},
    resource::ResourceState,
    value::{
        aggregates::{enumerations::EnumObject, tuples::TupleObject},
        carriers::ScopedValue,
        kind::ObjectKind,
        strings::StringObject,
    },
    value_semantics::storage_error,
};
use kagari_abi::representation::ValueType;
use kagari_gc::{access::Scope, error::GcError, object::Gc};
use kagari_types::language::binding;
use std::cmp::Ordering;

#[derive(Clone, Copy)]
enum Aggregate<'gc> {
    Tuple(Gc<'gc, TupleObject>),
    Enum(Gc<'gc, EnumObject>),
}

impl<'gc> Aggregate<'gc> {
    fn get(self, cx: &Scope<'gc>, index: usize) -> Result<ScopedValue<'gc>, RuntimeError> {
        match self {
            Self::Tuple(object) => TupleObject::get(cx, object, index),
            Self::Enum(object) => EnumObject::get(cx, object, index),
        }
        .map_err(storage_error)
    }
}

struct Members<'gc> {
    left: Aggregate<'gc>,
    right: Aggregate<'gc>,
    next: usize,
    count: usize,
}

enum Compared<'gc> {
    Equal,
    Unequal,
    Members(Members<'gc>),
}

pub(crate) fn identity_equal<'gc>(
    left: ScopedValue<'gc>,
    right: ScopedValue<'gc>,
) -> Result<bool, RuntimeError> {
    let admitted = |value: ScopedValue<'_>| {
        matches!(
            value.kind(),
            Some(ObjectKind::Struct | ObjectKind::Array | ObjectKind::Native)
        )
    };
    if !admitted(left) || !admitted(right) {
        return Err(TrapReason::IdentityComparisonRequiresObjectReferences.into());
    }
    Ok(left
        .object()
        .ok_or_else(|| storage_error(GcError::Representation))?
        .ptr_eq(
            right
                .object()
                .ok_or_else(|| storage_error(GcError::Representation))?,
        ))
}

/// Only unfinished siblings occupy scratch. Flat tuples and single-child chains
/// allocate no traversal stack; nested aggregates never recurse on the Rust stack.
pub(crate) fn script_equal<'gc>(
    cx: &Scope<'gc>,
    resources: &ResourceState,
    left: ScopedValue<'gc>,
    right: ScopedValue<'gc>,
) -> Result<bool, RuntimeError> {
    let mut current = match pair(cx, resources, left, right)? {
        Compared::Equal => return Ok(true),
        Compared::Unequal => return Ok(false),
        Compared::Members(members) => members,
    };
    let mut pending = Vec::new();
    let mut work = 0;
    loop {
        if current.next == current.count {
            match pending.pop() {
                Some(parent) => current = parent,
                None => return Ok(true),
            }
            continue;
        }
        work += 1;
        if work == 1024 {
            resources.poll_closed()?;
            work = 0;
        }
        let left = current.left.get(cx, current.next)?;
        let right = current.right.get(cx, current.next)?;
        current.next += 1;
        match pair(cx, resources, left, right)? {
            Compared::Equal => {}
            Compared::Unequal => return Ok(false),
            Compared::Members(child) => {
                if current.next != current.count {
                    pending
                        .try_reserve(1)
                        .map_err(|_| RuntimeError::resource_limit("value comparison scratch"))?;
                    pending.push(current);
                }
                current = child;
            }
        }
    }
}

fn pair<'gc>(
    cx: &Scope<'gc>,
    resources: &ResourceState,
    left: ScopedValue<'gc>,
    right: ScopedValue<'gc>,
) -> Result<Compared<'gc>, RuntimeError> {
    let unsupported = |value: ScopedValue<'_>| {
        matches!(
            value.kind(),
            Some(
                ObjectKind::Struct
                    | ObjectKind::Array
                    | ObjectKind::Native
                    | ObjectKind::Closure
                    | ObjectKind::Interface
                    | ObjectKind::HostRoot
                    | ObjectKind::HostPath
                    | ObjectKind::Ephemeral
            )
        )
    };
    if unsupported(left) || unsupported(right) {
        return Err(TrapReason::ValueCategoryDoesNotSupportGeneralEquality.into());
    }
    let equal =
        if let (Some((lt, a)), Some((rt, b))) = (left.scalar_payload(), right.scalar_payload()) {
            lt == rt
                && match lt {
                    ValueType::F32 => f32::from_bits(a as u32) == f32::from_bits(b as u32),
                    ValueType::F64 => f64::from_bits(a) == f64::from_bits(b),
                    _ => a == b,
                }
        } else {
            match (left.kind(), right.kind()) {
                (Some(ObjectKind::String), Some(ObjectKind::String)) => {
                    string_order(cx, resources, left, right)? == Ordering::Equal
                }
                (Some(ObjectKind::Tuple), Some(ObjectKind::Tuple)) => {
                    let a = cx
                        .downcast::<TupleObject>(
                            left.object()
                                .ok_or_else(|| storage_error(GcError::Representation))?,
                        )
                        .map_err(storage_error)?;
                    let b = cx
                        .downcast::<TupleObject>(
                            right
                                .object()
                                .ok_or_else(|| storage_error(GcError::Representation))?,
                        )
                        .map_err(storage_error)?;
                    let count = cx.read(a, TupleObject::len);
                    if count != cx.read(b, TupleObject::len) {
                        return Ok(Compared::Unequal);
                    }
                    if count != 0 {
                        return Ok(Compared::Members(Members {
                            left: Aggregate::Tuple(a),
                            right: Aggregate::Tuple(b),
                            next: 0,
                            count,
                        }));
                    }
                    true
                }
                (Some(ObjectKind::Enum), Some(ObjectKind::Enum)) => {
                    let a = cx
                        .downcast::<EnumObject>(
                            left.object()
                                .ok_or_else(|| storage_error(GcError::Representation))?,
                        )
                        .map_err(storage_error)?;
                    let b = cx
                        .downcast::<EnumObject>(
                            right
                                .object()
                                .ok_or_else(|| storage_error(GcError::Representation))?,
                        )
                        .map_err(storage_error)?;
                    if !cx.read(a, |a| cx.read(b, |b| a.layout() == b.layout())) {
                        return Ok(Compared::Unequal);
                    }
                    let count = cx.read(a, EnumObject::len);
                    if count != cx.read(b, EnumObject::len) {
                        return Ok(Compared::Unequal);
                    }
                    if count != 0 {
                        return Ok(Compared::Members(Members {
                            left: Aggregate::Enum(a),
                            right: Aggregate::Enum(b),
                            next: 0,
                            count,
                        }));
                    }
                    true
                }
                _ => false,
            }
        };
    Ok(if equal {
        Compared::Equal
    } else {
        Compared::Unequal
    })
}

pub(crate) fn builtin_order<'gc>(
    cx: &Scope<'gc>,
    resources: &ResourceState,
    left: ScopedValue<'gc>,
    right: ScopedValue<'gc>,
) -> Result<Option<Ordering>, RuntimeError> {
    let invalid = || RuntimeError::from(TrapReason::InvalidBuiltinOrderingOperands);
    if let (Some((lt, a)), Some((rt, b))) = (left.scalar_payload(), right.scalar_payload()) {
        if lt != rt {
            return Err(invalid());
        }
        return Ok(match lt {
            ValueType::Unit => Some(Ordering::Equal),
            ValueType::Bool | ValueType::Char | ValueType::U64 => a.partial_cmp(&b),
            ValueType::I32 | ValueType::I64 => (a as i64).partial_cmp(&(b as i64)),
            ValueType::F32 => f32::from_bits(a as u32).partial_cmp(&f32::from_bits(b as u32)),
            ValueType::F64 => f64::from_bits(a).partial_cmp(&f64::from_bits(b)),
            _ => return Err(invalid()),
        });
    }
    match (left.kind(), right.kind()) {
        (Some(ObjectKind::String), Some(ObjectKind::String)) => {
            string_order(cx, resources, left, right).map(Some)
        }
        (Some(ObjectKind::Enum), Some(ObjectKind::Enum)) => {
            let rank = |value: ScopedValue<'gc>| -> Result<usize, RuntimeError> {
                let object = cx
                    .downcast::<EnumObject>(value.object().ok_or_else(invalid)?)
                    .map_err(storage_error)?;
                cx.read(object, |object| {
                    let layout = object.layout();
                    if object.len() != 0
                        || !binding::matches(
                            &layout.layout().declaration,
                            &binding::ordering_declaration(),
                            layout.module().definitions(),
                        )
                    {
                        return None;
                    }
                    layout
                        .layout()
                        .variants
                        .iter()
                        .position(|variant| variant.declaration == layout.variant().declaration)
                })
                .ok_or_else(invalid)
            };
            Ok(Some(rank(left)?.cmp(&rank(right)?)))
        }
        _ => Err(invalid()),
    }
}

fn string_order<'gc>(
    cx: &Scope<'gc>,
    resources: &ResourceState,
    left: ScopedValue<'gc>,
    right: ScopedValue<'gc>,
) -> Result<Ordering, RuntimeError> {
    let a = cx
        .downcast::<StringObject>(
            left.object()
                .ok_or_else(|| storage_error(GcError::Representation))?,
        )
        .map_err(storage_error)?;
    let b = cx
        .downcast::<StringObject>(
            right
                .object()
                .ok_or_else(|| storage_error(GcError::Representation))?,
        )
        .map_err(storage_error)?;
    // Closed polling checks termination/cancellation without capturing a trace.
    // The outer semantic boundary admits the error after these payload loans end.
    cx.read(a, |a| {
        cx.read(b, |b| {
            let a = a.as_str().as_bytes();
            let b = b.as_str().as_bytes();
            for (index, (a, b)) in a.chunks(4096).zip(b.chunks(4096)).enumerate() {
                if index != 0 {
                    resources.poll_closed()?;
                }
                let order = a.cmp(b);
                if order != Ordering::Equal {
                    return Ok(order);
                }
            }
            Ok(a.len().cmp(&b.len()))
        })
    })
}

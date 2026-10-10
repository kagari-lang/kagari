//! Library members use ordinary checked declaration handles and pinned layouts.
use crate::declarations::StandardDeclarations;
use kagari_runtime::{
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    native::{binding::NativeResult, context::CallContext},
    value::{EnumTag, Value},
};
use std::cmp::Ordering;

pub(super) fn allocate(
    cx: &CallContext<'_>,
    ty: &TypeArgument,
    name: &str,
    member: &str,
    fields: Vec<Value>,
) -> NativeResult<Value> {
    let variant = StandardDeclarations::enumeration(name)?.variant(member)?;
    cx.allocate_enum(ty, &variant, fields)
}

/// Project checked immutable members into owned scalars before allocation/reentry.
/// The reader must not allocate on the script heap or invoke callbacks.
pub(super) fn read<R>(
    cx: &CallContext<'_>,
    value: &Value,
    name: &str,
    read: impl FnOnce(&str, &[Value]) -> R,
) -> NativeResult<R> {
    let Value::Enum(id) = value else {
        return Err(RuntimeError::module_validation("library enum value"));
    };
    let view = cx
        .heap()
        .enum_view(*id)
        .ok_or_else(|| RuntimeError::module_validation("library enum handle"))?;
    let EnumTag::Declared(layout) = &view.tag;
    let enumeration = StandardDeclarations::enumeration(name)?;
    let expected = layout
        .module()
        .definitions()
        .lookup(enumeration.id())
        .ok_or_else(|| RuntimeError::module_validation("library enum declaration"))?;
    if layout.layout().declaration != expected {
        return Err(RuntimeError::module_validation("foreign library enum"));
    }
    let member = layout
        .module()
        .definitions()
        .resolve(layout.variant().declaration)
        .ok()
        .and_then(|member| member.segments().last().map(|segment| segment.name))
        .ok_or_else(|| RuntimeError::module_validation("library enum member"))?;
    Ok(read(member, &view.fields))
}

/// Copy at most one payload; its caller roots it before allocation or reentry.
pub(super) fn option(cx: &CallContext<'_>, value: &Value) -> NativeResult<Option<Value>> {
    read(cx, value, "Option", |member, fields| {
        match (member, fields) {
            ("None", []) => Ok(None),
            ("Some", [value]) => Ok(Some(*value)),
            _ => Err(RuntimeError::module_validation("library Option member")),
        }
    })?
}

pub(super) fn ordering(cx: &CallContext<'_>, value: &Value) -> NativeResult<Ordering> {
    read(cx, value, "Ordering", |member, fields| {
        match (member, fields) {
            ("Less", []) => Ok(Ordering::Less),
            ("Equal", []) => Ok(Ordering::Equal),
            ("Greater", []) => Ok(Ordering::Greater),
            _ => Err(RuntimeError::module_validation("library Ordering member")),
        }
    })?
}

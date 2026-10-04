//! Library members use ordinary checked declaration handles and pinned layouts.
use crate::declarations::StandardDeclarations;
use kagari_runtime::{
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    native::{binding::NativeResult, context::CallContext},
    value::{EnumTag, Value},
};

pub(super) fn allocate(
    cx: &CallContext<'_>,
    ty: &TypeArgument,
    name: &str,
    member: &str,
    fields: Vec<Value>,
) -> NativeResult<Value> {
    let variant = StandardDeclarations::default()
        .enumeration(name)?
        .variant(member)?;
    cx.allocate_enum(ty, &variant, fields)
}

pub(super) fn inspect(
    cx: &CallContext<'_>,
    value: &Value,
    name: &str,
) -> NativeResult<(String, Vec<Value>)> {
    let Value::Enum(id) = value else {
        return Err(RuntimeError::module_validation("library enum value"));
    };
    let snapshot = cx
        .heap()
        .enum_snapshot(*id)
        .ok_or_else(|| RuntimeError::module_validation("library enum handle"))?;
    let EnumTag::Declared(layout) = snapshot.tag;
    let enumeration = StandardDeclarations::default().enumeration(name)?;
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
    Ok((member.to_owned(), snapshot.fields))
}

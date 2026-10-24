use crate::{
    frame::types::arguments::TypeArgument,
    gc::roots::RootedValue,
    native::{
        conversion::{IntoKagari, context::ConversionContext},
        value_handle::ScriptValue,
    },
    state::{
        codec::{scalar, schema, walk::Walk},
        data::StateValue,
        error::{PathComponent, SnapshotErrorKind, SnapshotResult},
    },
    value::{EnumTag, Value},
};

pub(super) fn restore(
    cx: &mut ConversionContext<'_>,
    expected: &TypeArgument,
    payload: &StateValue,
    walk: &mut Walk,
) -> SnapshotResult<ScriptValue> {
    let root = value(cx, expected, payload, 1, walk)?;
    let value = retained(cx, &root, walk)?;
    // Acquire the escaping program/value leases before the operation's last root
    // drops. No conversion-context scratch roots survive success or failure.
    let result = walk.checked(cx, |poll| {
        ScriptValue::from_kagari_with_poll(cx, expected, &value, poll)
    })?;
    walk.poll(cx)?;
    Ok(result)
}

fn value(
    cx: &mut ConversionContext<'_>,
    expected: &TypeArgument,
    payload: &StateValue,
    depth: usize,
    walk: &mut Walk,
) -> SnapshotResult<RootedValue> {
    let result = value_inner(cx, expected, payload, depth, walk);
    walk.locate(result)
}

fn value_inner(
    cx: &mut ConversionContext<'_>,
    expected: &TypeArgument,
    payload: &StateValue,
    depth: usize,
    walk: &mut Walk,
) -> SnapshotResult<RootedValue> {
    walk.poll(cx)?;
    walk.budget.node(depth)?;
    let raw = match payload {
        StateValue::Record { fields, .. } => record(cx, expected, fields, depth, walk)?,
        StateValue::Enum {
            variant, fields, ..
        } => enumeration(cx, expected, variant, fields, depth, walk)?,
        StateValue::Tuple(fields) => pack(cx, expected, fields, false, depth, walk)?,
        StateValue::Sequence(fields) => pack(cx, expected, fields, true, depth, walk)?,
        _ => scalar::restore(cx, expected, payload, walk)?,
    };
    walk.runtime(cx.check_value_shallow(expected, &raw))?;
    let root = cx
        .runtime()
        .root_value(raw)
        .ok_or_else(|| walk.error(SnapshotErrorKind::TypeMismatch, "restored value retention"))?;
    walk.runtime(cx.runtime().gc_safepoint())?;
    Ok(root)
}

fn retained(cx: &ConversionContext<'_>, root: &RootedValue, walk: &Walk) -> SnapshotResult<Value> {
    root.value(cx.runtime().gc())
        .ok_or_else(|| walk.error(SnapshotErrorKind::TypeMismatch, "expired restored child"))
}

fn record(
    cx: &mut ConversionContext<'_>,
    expected: &TypeArgument,
    fields: &[(String, StateValue)],
    depth: usize,
    walk: &mut Walk,
) -> SnapshotResult<Value> {
    walk.fields(fields.len())?;
    walk.budget.children(fields.len(), false)?;
    let layout = walk.runtime(cx.runtime().prepare_struct_layout(cx.owner(), expected))?;
    let object_type = walk.runtime(cx.runtime().retain_object_type(layout))?;
    let mut builder = walk.runtime(object_type.builder())?;
    for (name, payload) in fields {
        walk.poll(cx)?;
        schema::charge_fields(fields.len(), walk)?;
        let path_name = walk.copy_text(cx, name, true)?;
        let previous = walk.push(PathComponent::Field(path_name))?;
        let field = walk.runtime(object_type.field(name))?;
        let argument = walk.runtime(field.argument(cx.runtime()))?;
        let root = value(cx, &argument, payload, depth + 1, walk)?;
        let raw = retained(cx, &root, walk)?;
        walk.checked(cx, |poll| {
            builder.set_value_in_with_poll(cx, &field, raw, poll)
        })?;
        walk.pop(previous);
    }
    let object = walk.runtime(builder.build_in(cx))?;
    walk.runtime(object.into_kagari(cx, expected))
}

fn pack(
    cx: &mut ConversionContext<'_>,
    expected: &TypeArgument,
    fields: &[StateValue],
    sequence: bool,
    depth: usize,
    walk: &mut Walk,
) -> SnapshotResult<Value> {
    let mut values = walk.vector(fields.len(), false)?;
    let mut roots = walk.vector(fields.len(), false)?;
    let repeated = if sequence {
        Some(walk.runtime(cx.parameter(expected, 0))?)
    } else {
        None
    };
    for (index, payload) in fields.iter().enumerate() {
        let argument = match &repeated {
            Some(argument) => argument.clone(),
            None => walk.runtime(cx.parameter(expected, index))?,
        };
        let previous = walk.push(PathComponent::Index(index))?;
        let root = value(cx, &argument, payload, depth + 1, walk)?;
        values.push(retained(cx, &root, walk)?);
        roots.push(root);
        walk.pop(previous);
    }
    let result = walk.checked(cx, |poll| {
        if sequence {
            cx.runtime()
                .allocate_sequence_with_poll(cx.owner(), expected, values, poll)
        } else {
            cx.runtime().gc().alloc_tuple_with_poll(values, poll)
        }
    });
    // Child roots remain live through checked allocation. The caller immediately
    // retains the parent before a safepoint or any further heap operation.
    result
}

fn enumeration(
    cx: &mut ConversionContext<'_>,
    expected: &TypeArgument,
    variant: &str,
    fields: &[StateValue],
    depth: usize,
    walk: &mut Walk,
) -> SnapshotResult<Value> {
    walk.fields(fields.len())?;
    schema::charge_fields(walk.budget.limits.max_fields, walk)?;
    let layout = walk.runtime(
        cx.runtime()
            .declared_enum_variant(cx.owner(), expected, variant),
    )?;
    let arguments = walk.runtime(cx.variant_fields(&layout))?;
    if arguments.len() != fields.len() {
        return Err(walk.error(SnapshotErrorKind::TypeMismatch, "enum payload arity"));
    }
    let mut values = walk.vector(fields.len(), false)?;
    let mut roots = walk.vector(fields.len(), false)?;
    let name = walk.copy_text(cx, variant, true)?;
    let parent = walk.push(PathComponent::Variant(name))?;
    for (index, (payload, argument)) in fields.iter().zip(&arguments).enumerate() {
        let previous = walk.push(PathComponent::Index(index))?;
        let root = value(cx, argument, payload, depth + 1, walk)?;
        values.push(retained(cx, &root, walk)?);
        roots.push(root);
        walk.pop(previous);
    }
    walk.pop(parent);
    walk.runtime(
        cx.runtime()
            .alloc_enum(EnumTag::Declared(layout), values)
            .map(Value::Enum),
    )
}

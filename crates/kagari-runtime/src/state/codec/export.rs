use crate::{
    frame::types::arguments::TypeArgument,
    gc::{HeapObjectId, roots::RootSet},
    native::conversion::context::ConversionContext,
    state::{
        codec::{scalar, schema, walk::Walk},
        data::{NominalKey, StateSchema, StateShape, StateType, StateValue},
        error::{PathComponent, SnapshotErrorKind, SnapshotResult},
    },
    value::Value,
};
use std::{collections::HashMap, mem::size_of};

pub(super) fn export(
    cx: &mut ConversionContext<'_>,
    expected: &TypeArgument,
    value: &Value,
    schema: &StateSchema,
    walk: &mut Walk,
) -> SnapshotResult<StateValue> {
    Export {
        schema,
        walk,
        visited: HashMap::new(),
    }
    .value(cx, expected, value, &schema.root, 1)
}

struct Visit {
    path: Option<usize>,
    active: bool,
}

struct Export<'a> {
    schema: &'a StateSchema,
    walk: &'a mut Walk,
    visited: HashMap<HeapObjectId, Visit>,
}

impl Export<'_> {
    fn value(
        &mut self,
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
        value: &Value,
        ty: &StateType,
        depth: usize,
    ) -> SnapshotResult<StateValue> {
        let result = self.value_inner(cx, expected, value, ty, depth);
        self.walk.locate(result)
    }

    fn value_inner(
        &mut self,
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
        value: &Value,
        ty: &StateType,
        depth: usize,
    ) -> SnapshotResult<StateValue> {
        self.walk.poll(cx)?;
        self.walk.budget.node(depth)?;
        self.walk.runtime(cx.check_value_shallow(expected, value))?;
        let identity = match value {
            Value::Struct(id) | Value::Enum(id) | Value::Tuple(id) | Value::GcHandle(id) => {
                Some(*id)
            }
            _ => None,
        };
        if let Some(identity) = identity {
            if let Some(first) = self.visited.get(&identity) {
                let first_path = self.walk.path(first.path);
                let kind = if first.active {
                    SnapshotErrorKind::Cycle { first_path }
                } else {
                    SnapshotErrorKind::Alias { first_path }
                };
                return Err(self.walk.error(
                    kind,
                    "state snapshots require a tree of distinct compound objects",
                ));
            }
            self.visited.try_reserve(1).map_err(|_| {
                self.walk
                    .error(SnapshotErrorKind::Allocation, "state identity table")
            })?;
            self.visited.insert(
                identity,
                Visit {
                    path: self.walk.current,
                    active: true,
                },
            );
        }
        let result = match ty {
            StateType::Scalar(scalar) => scalar::export(cx, expected, value, *scalar, self.walk),
            StateType::Sequence(element) => self.sequence(cx, expected, value, element, depth),
            StateType::Tuple(fields) => self.tuple(cx, expected, value, fields, depth),
            StateType::Nominal(key) => self.nominal(cx, expected, value, key, depth),
        }?;
        if let Some(identity) = identity {
            self.visited
                .get_mut(&identity)
                .expect("entered identity")
                .active = false;
        }
        Ok(result)
    }

    fn child(
        &mut self,
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
        value: &Value,
        ty: &StateType,
        depth: usize,
        index: usize,
    ) -> SnapshotResult<StateValue> {
        let previous = self.walk.push(PathComponent::Index(index))?;
        let result = self.value(cx, expected, value, ty, depth + 1);
        self.walk.pop(previous);
        result
    }

    fn sequence(
        &mut self,
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
        value: &Value,
        element: &StateType,
        depth: usize,
    ) -> SnapshotResult<StateValue> {
        let Value::GcHandle(id) = value else {
            return Err(self
                .walk
                .error(SnapshotErrorKind::TypeMismatch, "expected nominal Vec"));
        };
        let count = cx.runtime().gc().sequence_len(*id).ok_or_else(|| {
            self.walk
                .error(SnapshotErrorKind::TypeMismatch, "expired Vec")
        })?;
        let mut values = self.walk.output(count, false)?;
        // Admission precedes the existing immediate-child snapshot allocation.
        let (roots, _) = self
            .walk
            .checked(cx, |poll| cx.snapshot_sequence_with_poll(value, poll))?;
        let argument = self.walk.runtime(cx.parameter(expected, 0))?;
        for index in 0..count {
            let value = self.root(cx, &roots, index)?;
            values.push(self.child(cx, &argument, &value, element, depth, index)?);
        }
        Ok(StateValue::Sequence(values))
    }

    fn tuple(
        &mut self,
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
        value: &Value,
        fields: &[StateType],
        depth: usize,
    ) -> SnapshotResult<StateValue> {
        let mut values = self.walk.output(fields.len(), false)?;
        let Value::Tuple(id) = value else {
            return Err(self
                .walk
                .error(SnapshotErrorKind::TypeMismatch, "expected tuple"));
        };
        let roots = {
            let items = cx.runtime().gc().tuple(*id).ok_or_else(|| {
                self.walk
                    .error(SnapshotErrorKind::TypeMismatch, "expired tuple")
            })?;
            if items.len() != fields.len() {
                return Err(self
                    .walk
                    .error(SnapshotErrorKind::TypeMismatch, "tuple arity"));
            }
            let mut snapshot = self.walk.vector(items.len(), false)?;
            for item in items.iter() {
                self.walk.poll(cx)?;
                self.walk.budget.work(size_of::<Value>() as u64)?;
                snapshot.push(*item);
            }
            self.walk.checked(cx, |poll| {
                cx.runtime()
                    .gc()
                    .root_execution_values_with_poll(snapshot, poll)
            })?
        };
        for (index, field) in fields.iter().enumerate() {
            let argument = self.walk.runtime(cx.parameter(expected, index))?;
            let value = self.root(cx, &roots, index)?;
            values.push(self.child(cx, &argument, &value, field, depth, index)?);
        }
        Ok(StateValue::Tuple(values))
    }

    fn root(
        &self,
        cx: &ConversionContext<'_>,
        roots: &RootSet,
        index: usize,
    ) -> SnapshotResult<Value> {
        roots.get(cx.runtime().gc(), index).ok_or_else(|| {
            self.walk
                .error(SnapshotErrorKind::TypeMismatch, "expired child snapshot")
        })
    }

    fn nominal(
        &mut self,
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
        value: &Value,
        key: &NominalKey,
        depth: usize,
    ) -> SnapshotResult<StateValue> {
        let definition = schema::lookup(cx, self.schema, key, self.walk)?;
        match &definition.shape {
            StateShape::Struct(fields) => {
                let key = schema::copy_key(cx, key, self.walk)?;
                let mut result = self.walk.output(fields.len(), false)?;
                let mut decoder = self.walk.runtime(cx.read_struct(expected, value))?;
                for field in fields {
                    schema::charge_fields(fields.len(), self.walk)?;
                    // Nested Result keeps codec kind/first-path diagnostics intact;
                    // the adapter continues owning all runtime access failures.
                    let name = self.walk.budget.copy_string(&field.name, true)?;
                    let path_name = self.walk.budget.copy_string(&field.name, true)?;
                    let previous = self.walk.push(PathComponent::Field(path_name))?;
                    let child = decoder.field_with(&field.name, |cx, expected, value| {
                        Ok(self.value(cx, expected, value, &field.ty, depth + 1))
                    });
                    let child = self.walk.runtime(child)??;
                    self.walk.pop(previous);
                    result.push((name, child));
                }
                Ok(StateValue::Record {
                    key,
                    fields: result,
                })
            }
            StateShape::Enum(variants) => {
                let layout = self.walk.runtime(cx.variant_layout(expected, value))?;
                let name = layout
                    .module()
                    .definition_name(layout.variant().declaration)
                    .ok_or_else(|| {
                        self.walk
                            .error(SnapshotErrorKind::TypeMismatch, "enum variant name")
                    })?;
                let mut selected = None;
                for variant in variants {
                    if self.walk.compare_text(cx, &variant.name, name)?.is_eq() {
                        selected = Some(variant);
                        break;
                    }
                }
                let selected = selected.ok_or_else(|| {
                    self.walk.error(
                        SnapshotErrorKind::UnknownMember,
                        "enum variant absent from checked schema",
                    )
                })?;
                let mut fields = self.walk.output(selected.fields.len(), false)?;
                let arguments = self.walk.runtime(cx.variant_fields(&layout))?;
                let Value::Enum(id) = value else {
                    unreachable!("variant_layout checked enum");
                };
                let snapshot = cx.runtime().gc().enum_snapshot(*id).ok_or_else(|| {
                    self.walk
                        .error(SnapshotErrorKind::TypeMismatch, "enum snapshot")
                })?;
                if snapshot.fields.len() != selected.fields.len()
                    || arguments.len() != selected.fields.len()
                {
                    return Err(self
                        .walk
                        .error(SnapshotErrorKind::TypeMismatch, "enum payload arity"));
                }
                let roots = self.walk.checked(cx, |poll| {
                    cx.runtime()
                        .gc()
                        .root_execution_values_with_poll(snapshot.fields, poll)
                })?;
                let variant = self.walk.copy_text(cx, name, true)?;
                let previous = self.walk.push(PathComponent::Variant(variant.clone()))?;
                for (index, (argument, ty)) in arguments.iter().zip(&selected.fields).enumerate() {
                    let value = self.root(cx, &roots, index)?;
                    fields.push(self.child(cx, argument, &value, ty, depth, index)?);
                }
                self.walk.pop(previous);
                Ok(StateValue::Enum {
                    key: schema::copy_key(cx, key, self.walk)?,
                    variant,
                    fields,
                })
            }
        }
    }
}

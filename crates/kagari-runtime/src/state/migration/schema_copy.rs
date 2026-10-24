use crate::state::{
    data::{
        NominalKey, SchemaVersion, StateDefinition, StateField, StateSchema, StateShape, StateType,
        StateVariant,
    },
    error::SnapshotResult,
    limits::Budget,
};

fn list<T, U>(
    values: &[T],
    budget: &mut Budget,
    mut copy: impl FnMut(&T, &mut Budget) -> SnapshotResult<U>,
) -> SnapshotResult<Vec<U>> {
    budget.children(values.len(), true)?;
    let mut output = Vec::new();
    budget.reserve(&mut output, values.len())?;
    for value in values {
        budget.work(1)?;
        output.push(copy(value, budget)?);
    }
    Ok(output)
}

fn key(value: &NominalKey, depth: usize, budget: &mut Budget) -> SnapshotResult<NominalKey> {
    budget.schema_node(depth)?;
    Ok(NominalKey {
        package: budget.copy_string(&value.package, true)?,
        module: list(&value.module, budget, |name, budget| {
            budget.copy_string(name, true)
        })?,
        declaration: list(&value.declaration, budget, |name, budget| {
            budget.copy_string(name, true)
        })?,
        arguments: list(&value.arguments, budget, |value, budget| {
            ty(value, depth + 1, budget)
        })?,
    })
}

fn ty(value: &StateType, depth: usize, budget: &mut Budget) -> SnapshotResult<StateType> {
    budget.schema_node(depth)?;
    Ok(match value {
        StateType::Scalar(value) => StateType::Scalar(*value),
        StateType::Tuple(values) => StateType::Tuple(list(values, budget, |value, budget| {
            ty(value, depth + 1, budget)
        })?),
        StateType::Sequence(value) => StateType::Sequence(Box::new(ty(value, depth + 1, budget)?)),
        StateType::Nominal(value) => StateType::Nominal(key(value, depth, budget)?),
    })
}

pub(super) fn copy(value: &StateSchema, budget: &mut Budget) -> SnapshotResult<StateSchema> {
    Ok(StateSchema {
        version: SchemaVersion {
            id: budget.copy_string(&value.version.id, true)?,
            version: value.version.version,
        },
        root: ty(&value.root, 1, budget)?,
        definitions: list(&value.definitions, budget, |definition, budget| {
            budget.schema_node(1)?;
            Ok(StateDefinition {
                key: key(&definition.key, 1, budget)?,
                shape: match &definition.shape {
                    StateShape::Struct(fields) => {
                        StateShape::Struct(list(fields, budget, |field, budget| {
                            budget.schema_node(1)?;
                            Ok(StateField {
                                name: budget.copy_string(&field.name, true)?,
                                ty: ty(&field.ty, 1, budget)?,
                            })
                        })?)
                    }
                    StateShape::Enum(variants) => {
                        StateShape::Enum(list(variants, budget, |variant, budget| {
                            budget.schema_node(1)?;
                            Ok(StateVariant {
                                name: budget.copy_string(&variant.name, true)?,
                                fields: list(&variant.fields, budget, |value, budget| {
                                    ty(value, 1, budget)
                                })?,
                            })
                        })?)
                    }
                },
            })
        })?,
    })
}

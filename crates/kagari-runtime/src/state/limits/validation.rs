use crate::state::{
    data::{
        NominalKey, SchemaVersion, SnapshotHeader, StateDefinition, StateScalar, StateSchema,
        StateShape, StateType, StateValue,
    },
    error::{PathComponent, SnapshotError, SnapshotErrorKind, SnapshotPath, SnapshotResult},
    limits::Budget,
    wire,
};
use std::cmp::Ordering;

fn version(value: &SchemaVersion, budget: &mut Budget) -> SnapshotResult<()> {
    identifier(&value.id, budget)?;
    if value.version == 0 {
        return Err(budget.error(
            SnapshotErrorKind::InvalidSchema,
            "schema version must be nonzero",
        ));
    }
    Ok(())
}

fn identifier(value: &str, budget: &mut Budget) -> SnapshotResult<()> {
    budget.string(value, true)?;
    if value.is_empty() {
        return Err(budget.error(SnapshotErrorKind::InvalidSchema, "empty identifier"));
    }
    Ok(())
}

fn key(value: &NominalKey, depth: usize, budget: &mut Budget) -> SnapshotResult<()> {
    budget.schema_node(depth)?;
    identifier(&value.package, budget)?;
    if value.declaration.is_empty() {
        return Err(budget.error(SnapshotErrorKind::InvalidSchema, "empty declaration path"));
    }
    for names in [&value.module, &value.declaration] {
        budget.children(names.len(), true)?;
        for name in names {
            budget.schema_node(depth)?;
            identifier(name, budget)?;
        }
    }
    budget.children(value.arguments.len(), true)?;
    for argument in &value.arguments {
        ty(argument, depth + 1, budget)?;
    }
    Ok(())
}

fn ty(value: &StateType, depth: usize, budget: &mut Budget) -> SnapshotResult<()> {
    budget.schema_node(depth)?;
    match value {
        StateType::Scalar(_) => Ok(()),
        StateType::Sequence(element) => ty(element, depth + 1, budget),
        StateType::Tuple(fields) => {
            budget.children(fields.len(), true)?;
            for field in fields {
                ty(field, depth + 1, budget)?;
            }
            Ok(())
        }
        StateType::Nominal(value) => key(value, depth, budget),
    }
}

fn names<'a>(values: impl Iterator<Item = &'a str>, budget: &mut Budget) -> SnapshotResult<()> {
    let mut previous = None;
    for name in values {
        identifier(name, budget)?;
        if let Some(previous) = previous {
            ordered(compare_text(previous, name, budget)?, budget)?;
        }
        previous = Some(name);
    }
    Ok(())
}

fn ordered(order: Ordering, budget: &Budget) -> SnapshotResult<()> {
    match order {
        Ordering::Less => Ok(()),
        Ordering::Equal => Err(budget.error(SnapshotErrorKind::Duplicate, "duplicate name or key")),
        Ordering::Greater => Err(budget.error(
            SnapshotErrorKind::NonCanonical,
            "names and keys must be strictly ordered",
        )),
    }
}

fn compare_text(a: &str, b: &str, budget: &mut Budget) -> SnapshotResult<Ordering> {
    // Charge before comparison, and perform long comparisons in cancellation chunks.
    for (left, right) in a.as_bytes().chunks(4096).zip(b.as_bytes().chunks(4096)) {
        budget.bytes(left.len().max(right.len()))?;
        let common = left.len().min(right.len());
        let order = left[..common].cmp(&right[..common]);
        if order != Ordering::Equal {
            return Ok(order);
        }
    }
    Ok(a.len().cmp(&b.len()))
}

fn compare_types(a: &StateType, b: &StateType, budget: &mut Budget) -> SnapshotResult<Ordering> {
    budget.work(1)?;
    let rank = |value: &StateType| match value {
        StateType::Scalar(_) => 0,
        StateType::Tuple(_) => 1,
        StateType::Sequence(_) => 2,
        StateType::Nominal(_) => 3,
    };
    match (a, b) {
        (StateType::Scalar(a), StateType::Scalar(b)) => Ok(a.cmp(b)),
        (StateType::Tuple(a), StateType::Tuple(b)) => compare_arguments(a, b, budget),
        (StateType::Sequence(a), StateType::Sequence(b)) => compare_types(a, b, budget),
        (StateType::Nominal(a), StateType::Nominal(b)) => compare_keys(a, b, budget),
        _ => Ok(rank(a).cmp(&rank(b))),
    }
}

fn compare_arguments(
    a: &[StateType],
    b: &[StateType],
    budget: &mut Budget,
) -> SnapshotResult<Ordering> {
    for (a, b) in a.iter().zip(b) {
        let order = compare_types(a, b, budget)?;
        if order != Ordering::Equal {
            return Ok(order);
        }
    }
    Ok(a.len().cmp(&b.len()))
}

fn compare_keys(a: &NominalKey, b: &NominalKey, budget: &mut Budget) -> SnapshotResult<Ordering> {
    budget.work(1)?;
    let order = compare_text(&a.package, &b.package, budget)?;
    if order != Ordering::Equal {
        return Ok(order);
    }
    for (left, right) in [(&a.module, &b.module), (&a.declaration, &b.declaration)] {
        for (a, b) in left.iter().zip(right) {
            let order = compare_text(a, b, budget)?;
            if order != Ordering::Equal {
                return Ok(order);
            }
        }
        let order = left.len().cmp(&right.len());
        if order != Ordering::Equal {
            return Ok(order);
        }
    }
    compare_arguments(&a.arguments, &b.arguments, budget)
}

fn lookup(
    key: &NominalKey,
    definitions: &[StateDefinition],
    budget: &mut Budget,
) -> SnapshotResult<usize> {
    let (mut start, mut end) = (0, definitions.len());
    while start < end {
        let middle = start + (end - start) / 2;
        match compare_keys(key, &definitions[middle].key, budget)? {
            Ordering::Equal => return Ok(middle),
            Ordering::Less => end = middle,
            Ordering::Greater => start = middle + 1,
        }
    }
    Err(budget.error(
        SnapshotErrorKind::InvalidSchema,
        "dangling nominal reference",
    ))
}

fn references(
    ty: &StateType,
    definitions: &[StateDefinition],
    reachable: &mut [bool],
    pending: &mut Vec<usize>,
    budget: &mut Budget,
) -> SnapshotResult<()> {
    budget.work(1)?;
    match ty {
        StateType::Scalar(_) => {}
        StateType::Tuple(fields) => {
            for field in fields {
                references(field, definitions, reachable, pending, budget)?;
            }
        }
        StateType::Sequence(element) => {
            references(element, definitions, reachable, pending, budget)?
        }
        StateType::Nominal(key) => {
            let index = lookup(key, definitions, budget)?;
            if !reachable[index] {
                reachable[index] = true;
                pending.push(index);
            }
            for argument in &key.arguments {
                references(argument, definitions, reachable, pending, budget)?;
            }
        }
    }
    Ok(())
}

pub(super) fn schema(schema: &StateSchema, budget: &mut Budget) -> SnapshotResult<()> {
    version(&schema.version, budget)?;
    budget.count(
        schema.definitions.len(),
        budget.limits.max_definitions,
        "definitions",
    )?;
    budget.children(schema.definitions.len(), true)?;
    ty(&schema.root, 1, budget)?;
    for (index, definition) in schema.definitions.iter().enumerate() {
        let result = (|| {
            budget.schema_node(1)?;
            key(&definition.key, 1, budget)?;
            if index > 0 {
                ordered(
                    compare_keys(&schema.definitions[index - 1].key, &definition.key, budget)?,
                    budget,
                )?;
            }
            match &definition.shape {
                StateShape::Struct(fields) => {
                    budget.count(fields.len(), budget.limits.max_fields, "fields")?;
                    budget.children(fields.len(), true)?;
                    names(fields.iter().map(|field| field.name.as_str()), budget)?;
                    for field in fields {
                        budget.schema_node(1)?;
                        ty(&field.ty, 1, budget)?;
                    }
                }
                StateShape::Enum(variants) => {
                    budget.count(variants.len(), budget.limits.max_fields, "variants")?;
                    budget.children(variants.len(), true)?;
                    names(variants.iter().map(|variant| variant.name.as_str()), budget)?;
                    for variant in variants {
                        budget.schema_node(1)?;
                        budget.count(variant.fields.len(), budget.limits.max_fields, "fields")?;
                        budget.children(variant.fields.len(), true)?;
                        for field in &variant.fields {
                            ty(field, 1, budget)?;
                        }
                    }
                }
            }
            Ok(())
        })();
        result.map_err(|error: SnapshotError| {
            error.at(&SnapshotPath(vec![PathComponent::Definition(index)]))
        })?;
    }
    let mut reachable = Vec::new();
    budget.reserve(&mut reachable, schema.definitions.len())?;
    reachable.resize(schema.definitions.len(), false);
    let mut pending = Vec::new();
    budget.reserve(&mut pending, schema.definitions.len())?;
    references(
        &schema.root,
        &schema.definitions,
        &mut reachable,
        &mut pending,
        budget,
    )?;
    while let Some(index) = pending.pop() {
        let definition = &schema.definitions[index];
        for argument in &definition.key.arguments {
            references(
                argument,
                &schema.definitions,
                &mut reachable,
                &mut pending,
                budget,
            )?;
        }
        match &definition.shape {
            StateShape::Struct(fields) => {
                for field in fields {
                    references(
                        &field.ty,
                        &schema.definitions,
                        &mut reachable,
                        &mut pending,
                        budget,
                    )?;
                }
            }
            StateShape::Enum(variants) => {
                for variant in variants {
                    for field in &variant.fields {
                        references(
                            field,
                            &schema.definitions,
                            &mut reachable,
                            &mut pending,
                            budget,
                        )?;
                    }
                }
            }
        }
    }
    if reachable.contains(&false) {
        return Err(budget.error(
            SnapshotErrorKind::InvalidSchema,
            "unreachable schema definition",
        ));
    }
    Ok(())
}

fn scalar(value: &StateValue, expected: StateScalar) -> bool {
    matches!(
        (value, expected),
        (StateValue::Unit, StateScalar::Unit)
            | (StateValue::Bool(_), StateScalar::Bool)
            | (StateValue::Char(_), StateScalar::Char)
            | (StateValue::I8(_), StateScalar::I8)
            | (StateValue::I16(_), StateScalar::I16)
            | (StateValue::I32(_), StateScalar::I32)
            | (StateValue::I64(_), StateScalar::I64)
            | (StateValue::U8(_), StateScalar::U8)
            | (StateValue::U16(_), StateScalar::U16)
            | (StateValue::U32(_), StateScalar::U32)
            | (StateValue::U64(_), StateScalar::U64)
            | (StateValue::F32Bits(_), StateScalar::F32)
            | (StateValue::F64Bits(_), StateScalar::F64)
            | (StateValue::String(_), StateScalar::String)
    )
}

fn payload(
    value: &StateValue,
    expected: &StateType,
    definitions: &[StateDefinition],
    depth: usize,
    path: &mut SnapshotPath,
    budget: &mut Budget,
) -> SnapshotResult<()> {
    let result = (|| {
        budget.node(depth)?;
        match (value, expected) {
            (value, StateType::Scalar(expected)) if scalar(value, *expected) => {
                if let StateValue::String(value) = value {
                    budget.string(value, false)?;
                }
            }
            (StateValue::Tuple(values), StateType::Tuple(types)) => {
                positional(values, types, definitions, depth, path, budget)?;
            }
            (StateValue::Sequence(values), StateType::Sequence(ty)) => {
                budget.children(values.len(), false)?;
                for (index, value) in values.iter().enumerate() {
                    path.push(PathComponent::Index(index));
                    let result = payload(value, ty, definitions, depth + 1, path, budget);
                    path.pop();
                    result?;
                }
            }
            (
                StateValue::Record {
                    key: actual,
                    fields,
                },
                StateType::Nominal(expected),
            ) => {
                key(actual, 1, budget)?;
                if compare_keys(actual, expected, budget)? != Ordering::Equal {
                    return Err(budget.error(
                        SnapshotErrorKind::TypeMismatch,
                        "record nominal key differs",
                    ));
                }
                let definition = &definitions[lookup(expected, definitions, budget)?];
                let StateShape::Struct(declared) = &definition.shape else {
                    return Err(budget.error(
                        SnapshotErrorKind::TypeMismatch,
                        "expected enum, received record",
                    ));
                };
                budget.count(fields.len(), budget.limits.max_fields, "fields")?;
                budget.children(fields.len(), false)?;
                names(fields.iter().map(|(name, _)| name.as_str()), budget)?;
                for (name, value) in fields {
                    // The bounded field table is searched explicitly and charged, not hidden.
                    let mut found = None;
                    for field in declared {
                        budget.work(1)?;
                        if compare_text(name, &field.name, budget)? == Ordering::Equal {
                            found = Some(field);
                            break;
                        }
                    }
                    let field = found.ok_or_else(|| {
                        budget.error(SnapshotErrorKind::UnknownMember, "unknown record field")
                    })?;
                    path.push(PathComponent::Field(name.clone()));
                    let result = payload(value, &field.ty, definitions, depth + 1, path, budget);
                    path.pop();
                    result?;
                }
                if fields.len() != declared.len() {
                    return Err(
                        budget.error(SnapshotErrorKind::MissingMember, "missing record field")
                    );
                }
            }
            (
                StateValue::Enum {
                    key: actual,
                    variant,
                    fields,
                },
                StateType::Nominal(expected),
            ) => {
                key(actual, 1, budget)?;
                identifier(variant, budget)?;
                if compare_keys(actual, expected, budget)? != Ordering::Equal {
                    return Err(
                        budget.error(SnapshotErrorKind::TypeMismatch, "enum nominal key differs")
                    );
                }
                let definition = &definitions[lookup(expected, definitions, budget)?];
                let StateShape::Enum(variants) = &definition.shape else {
                    return Err(budget.error(
                        SnapshotErrorKind::TypeMismatch,
                        "expected record, received enum",
                    ));
                };
                let mut found = None;
                for candidate in variants {
                    budget.work(1)?;
                    if compare_text(variant, &candidate.name, budget)? == Ordering::Equal {
                        found = Some(candidate);
                        break;
                    }
                }
                let declared = found.ok_or_else(|| {
                    budget.error(SnapshotErrorKind::UnknownMember, "unknown enum variant")
                })?;
                path.push(PathComponent::Variant(variant.clone()));
                let result = positional(fields, &declared.fields, definitions, depth, path, budget);
                path.pop();
                result?;
            }
            _ => {
                return Err(budget.error(
                    SnapshotErrorKind::TypeMismatch,
                    "payload kind does not match the exact schema type",
                ));
            }
        }
        Ok(())
    })();
    result.map_err(|error: SnapshotError| {
        if error.path.0.is_empty() {
            error.at(path)
        } else {
            error
        }
    })
}

fn positional(
    values: &[StateValue],
    types: &[StateType],
    definitions: &[StateDefinition],
    depth: usize,
    path: &mut SnapshotPath,
    budget: &mut Budget,
) -> SnapshotResult<()> {
    budget.children(values.len(), false)?;
    if values.len() != types.len() {
        return Err(budget.error(
            SnapshotErrorKind::TypeMismatch,
            "positional field count differs",
        ));
    }
    for (index, (value, ty)) in values.iter().zip(types).enumerate() {
        path.push(PathComponent::Index(index));
        let result = payload(value, ty, definitions, depth + 1, path, budget);
        path.pop();
        result?;
    }
    Ok(())
}

pub(super) fn versions_equal(
    a: &SchemaVersion,
    b: &SchemaVersion,
    budget: &mut Budget,
) -> SnapshotResult<bool> {
    Ok(a.version == b.version && compare_text(&a.id, &b.id, budget)? == Ordering::Equal)
}

pub(super) fn schemas_equal(
    a: &StateSchema,
    b: &StateSchema,
    budget: &mut Budget,
) -> SnapshotResult<bool> {
    if a.version.version != b.version.version
        || compare_text(&a.version.id, &b.version.id, budget)? != Ordering::Equal
        || a.definitions.len() != b.definitions.len()
        || compare_types(&a.root, &b.root, budget)? != Ordering::Equal
    {
        return Ok(false);
    }
    for (a, b) in a.definitions.iter().zip(&b.definitions) {
        if compare_keys(&a.key, &b.key, budget)? != Ordering::Equal {
            return Ok(false);
        }
        match (&a.shape, &b.shape) {
            (StateShape::Struct(a), StateShape::Struct(b)) if a.len() == b.len() => {
                for (a, b) in a.iter().zip(b) {
                    budget.work(1)?;
                    if compare_text(&a.name, &b.name, budget)? != Ordering::Equal
                        || compare_types(&a.ty, &b.ty, budget)? != Ordering::Equal
                    {
                        return Ok(false);
                    }
                }
            }
            (StateShape::Enum(a), StateShape::Enum(b)) if a.len() == b.len() => {
                for (a, b) in a.iter().zip(b) {
                    budget.work(1)?;
                    if compare_text(&a.name, &b.name, budget)? != Ordering::Equal
                        || compare_arguments(&a.fields, &b.fields, budget)? != Ordering::Equal
                    {
                        return Ok(false);
                    }
                }
            }
            _ => return Ok(false),
        }
    }
    Ok(true)
}

pub(super) fn snapshot(
    header: &SnapshotHeader,
    schema_value: &StateSchema,
    value: &StateValue,
    budget: &mut Budget,
) -> SnapshotResult<()> {
    version(&header.schema, budget)?;
    budget.string(&header.source_deployment, true)?;
    budget.string(&header.state_owner, true)?;
    schema(schema_value, budget)?;
    if !versions_equal(&header.schema, &schema_value.version, budget)? {
        return Err(budget.error(
            SnapshotErrorKind::SchemaMismatch,
            "header and embedded schema versions differ",
        ));
    }
    payload(
        value,
        &schema_value.root,
        &schema_value.definitions,
        1,
        &mut SnapshotPath::root(),
        budget,
    )?;
    wire::check_size(header, schema_value, value, budget)
}

use crate::{
    frame::types::arguments::TypeArgument,
    native::{collections::vector::storage::declaration, conversion::context::ConversionContext},
    state::{
        codec::{scalar, walk::Walk},
        data::{
            NominalKey, SchemaVersion, StateDefinition, StateField, StateSchema, StateShape,
            StateType, StateVariant,
        },
        error::{PathComponent, SnapshotErrorKind, SnapshotResult},
    },
};
use kagari_common::identity::DefinitionKind;
use kagari_contract::types::PublicItem;
use kagari_types::{declaration::TypeDefKind, language::binding, ty::Ty};
use std::cmp::Ordering;

mod compare;

pub(super) fn describe(
    cx: &ConversionContext<'_>,
    expected: &TypeArgument,
    version: SchemaVersion,
    walk: &mut Walk,
) -> SnapshotResult<StateSchema> {
    walk.poll(cx)?;
    walk.runtime(expected.validate_for(cx.runtime(), cx.owner()))?;
    walk.budget.string(&version.id, true)?;
    let mut builder = Builder {
        cx,
        walk,
        definitions: Vec::new(),
        pending: Vec::new(),
    };
    let root = builder.ty(expected, 1)?;
    let mut index = 0;
    // A queue, rather than recursive shape expansion, bounds mutually recursive
    // nominal schemas and allows chains whose payloads are empty at runtime.
    while index < builder.pending.len() {
        builder.walk.poll(cx)?;
        let expected = builder.pending[index].clone();
        let previous = builder.walk.push(PathComponent::Definition(index))?;
        let shape = builder.shape(&expected);
        let shape = builder.walk.locate(shape)?;
        builder.walk.pop(previous);
        builder.definitions[index].shape = shape;
        index += 1;
    }
    for index in 1..builder.definitions.len() {
        let mut slot = index;
        while slot > 0
            && compare::key(
                cx,
                &builder.definitions[slot - 1].key,
                &builder.definitions[slot].key,
                builder.walk,
            )?
            .is_gt()
        {
            builder.walk.budget.work(1)?;
            builder.definitions.swap(slot - 1, slot);
            slot -= 1;
        }
    }
    Ok(StateSchema {
        version,
        root,
        definitions: builder.definitions,
    })
}

struct Builder<'a, 'runtime> {
    cx: &'a ConversionContext<'runtime>,
    walk: &'a mut Walk,
    definitions: Vec<StateDefinition>,
    pending: Vec<TypeArgument>,
}

impl Builder<'_, '_> {
    fn ty(&mut self, expected: &TypeArgument, depth: usize) -> SnapshotResult<StateType> {
        self.walk.poll(self.cx)?;
        self.walk.budget.schema_node(depth)?;
        self.walk
            .runtime(expected.validate_for(self.cx.runtime(), self.cx.owner()))?;
        if let Some(scalar) = scalar::describe(expected.ty()) {
            return Ok(StateType::Scalar(scalar));
        }
        match expected.ty() {
            Ty::Tuple(fields) => {
                let mut result = self.walk.output(fields.len(), true)?;
                for index in 0..fields.len() {
                    let argument = self.walk.runtime(self.cx.parameter(expected, index))?;
                    result.push(self.ty(&argument, depth + 1)?);
                }
                Ok(StateType::Tuple(result))
            }
            Ty::NativeObject(nominal)
                if binding::matches(
                    &nominal.declaration,
                    &declaration(),
                    expected.definitions(),
                ) && nominal.arguments.len() == 1
                    && nominal.associated_types.is_empty() =>
            {
                // Identity comes from the installed standard declaration, never its name.
                self.walk.runtime(
                    self.cx
                        .runtime()
                        .native_entries
                        .catalog
                        .type_reference(&declaration()),
                )?;
                let argument = self.walk.runtime(self.cx.parameter(expected, 0))?;
                Ok(StateType::Sequence(Box::new(
                    self.ty(&argument, depth + 1)?,
                )))
            }
            Ty::Struct(nominal) | Ty::Enum(nominal) if nominal.associated_types.is_empty() => {
                let definition = expected
                    .definitions()
                    .resolve(nominal.declaration)
                    .map_err(|_| {
                        self.walk.error(
                            SnapshotErrorKind::UnsupportedType,
                            "unresolved nominal declaration",
                        )
                    })?;
                let mut key = NominalKey {
                    package: self
                        .walk
                        .copy_text(self.cx, &definition.module().package.0, true)?,
                    module: self.walk.output(definition.module().path.len(), true)?,
                    declaration: Vec::new(),
                    arguments: self.walk.output(nominal.arguments.len(), true)?,
                };
                for part in &definition.module().path {
                    key.module.push(self.walk.copy_text(self.cx, part, true)?);
                }
                for part in definition.segments() {
                    if part.occurrence != 0
                        || !matches!(
                            part.kind,
                            DefinitionKind::Struct | DefinitionKind::Enum | DefinitionKind::Module
                        )
                    {
                        return Err(self.walk.error(
                            SnapshotErrorKind::UnsupportedType,
                            "state requires an ordinary named declaration",
                        ));
                    }
                    self.walk.budget.schema_node(depth)?;
                    self.walk.budget.reserve(&mut key.declaration, 1)?;
                    key.declaration
                        .push(self.walk.copy_text(self.cx, part.name, true)?);
                }
                for index in 0..nominal.arguments.len() {
                    let argument = self.walk.runtime(self.cx.parameter(expected, index))?;
                    key.arguments.push(self.ty(&argument, depth + 1)?);
                }
                let mut found = false;
                for definition in &self.definitions {
                    if compare::key(self.cx, &definition.key, &key, self.walk)?.is_eq() {
                        found = true;
                        break;
                    }
                }
                if !found {
                    self.walk.budget.count(
                        self.definitions.len() + 1,
                        self.walk.budget.limits.max_definitions,
                        "definitions",
                    )?;
                    self.walk.budget.reserve(&mut self.definitions, 1)?;
                    self.walk.budget.reserve(&mut self.pending, 1)?;
                    self.definitions.push(StateDefinition {
                        key: copy_key(self.cx, &key, self.walk)?,
                        shape: StateShape::Struct(Vec::new()),
                    });
                    self.pending.push(expected.clone());
                }
                Ok(StateType::Nominal(key))
            }
            _ => Err(self.walk.error(
                SnapshotErrorKind::UnsupportedType,
                "type is not supported by the state tree codec",
            )),
        }
    }

    fn shape(&mut self, expected: &TypeArgument) -> SnapshotResult<StateShape> {
        match expected.ty() {
            Ty::Struct(_) => {
                let layout = self.walk.runtime(
                    self.cx
                        .runtime()
                        .prepare_struct_layout(self.cx.owner(), expected),
                )?;
                self.walk.fields(layout.layout().fields.len())?;
                let object = self
                    .walk
                    .runtime(self.cx.runtime().retain_object_type(layout.clone()))?;
                // This also rejects private zero-field types and inaccessible fields.
                let _builder = self.walk.runtime(object.builder())?;
                let mut fields = self.walk.output(layout.layout().fields.len(), true)?;
                for field in &layout.layout().fields {
                    self.walk.poll(self.cx)?;
                    charge_fields(layout.layout().fields.len(), self.walk)?;
                    let field_type = self.walk.runtime(object.field(&field.name))?;
                    let argument = self.walk.runtime(field_type.argument(self.cx.runtime()))?;
                    let name = self.walk.copy_text(self.cx, &field.name, true)?;
                    let previous = self.walk.push(PathComponent::Field(name.clone()))?;
                    let result = self.ty(&argument, 1);
                    let ty = self.walk.locate(result)?;
                    self.walk.pop(previous);
                    fields.push(StateField { name, ty });
                }
                sort_names(self.cx, &mut fields, |field| &field.name, self.walk)?;
                Ok(StateShape::Struct(fields))
            }
            Ty::Enum(nominal) => {
                let definition = expected
                    .definitions()
                    .resolve(nominal.declaration)
                    .map_err(|_| {
                        self.walk
                            .error(SnapshotErrorKind::UnsupportedType, "enum declaration")
                    })?;
                let mut parts = definition.segments();
                let part = parts.next().ok_or_else(|| {
                    self.walk
                        .error(SnapshotErrorKind::UnsupportedType, "enum declaration")
                })?;
                if parts.next().is_some()
                    || part.kind != DefinitionKind::Enum
                    || part.occurrence != 0
                {
                    return Err(self.walk.error(
                        SnapshotErrorKind::UnsupportedType,
                        "enum must be an ordinary public declaration",
                    ));
                }
                let mut public = false;
                for member in self.cx.owner().members() {
                    self.walk.poll(self.cx)?;
                    if &member.bytecode.identity != definition.module() {
                        continue;
                    }
                    for item in &member.bytecode.public_items {
                        self.walk.budget.work(1)?;
                        if let PublicItem::Type(ty) = item
                            && ty.kind == TypeDefKind::Enum
                            && self
                                .walk
                                .compare_text(self.cx, &ty.name, part.name)?
                                .is_eq()
                            && ty.generic_params.len() == nominal.arguments.len()
                        {
                            public = true;
                            break;
                        }
                    }
                }
                if !public {
                    return Err(self
                        .walk
                        .error(SnapshotErrorKind::UnsupportedType, "enum is not public"));
                }
                let first = self.walk.runtime(
                    self.cx
                        .runtime()
                        .prepare_enum_layout(self.cx.owner(), expected),
                )?;
                let Some(first) = first else {
                    return Ok(StateShape::Enum(Vec::new()));
                };
                let count = first.layout().variants.len();
                self.walk.fields(count)?;
                let mut variants = self.walk.output(count, true)?;
                for index in 0..count {
                    self.walk.poll(self.cx)?;
                    let layout = first.with_variant(index as u32).ok_or_else(|| {
                        self.walk
                            .error(SnapshotErrorKind::InvalidSchema, "enum variant layout")
                    })?;
                    self.walk.fields(layout.variant().payload.len())?;
                    let name = layout
                        .module()
                        .definition_name(layout.variant().declaration)
                        .ok_or_else(|| {
                            self.walk
                                .error(SnapshotErrorKind::InvalidSchema, "enum variant name")
                        })?;
                    let name = self.walk.copy_text(self.cx, name, true)?;
                    let arguments = self.walk.runtime(self.cx.variant_fields(&layout))?;
                    let mut fields = self.walk.output(arguments.len(), true)?;
                    let previous = self.walk.push(PathComponent::Variant(name.clone()))?;
                    for argument in arguments {
                        let result = self.ty(&argument, 1);
                        fields.push(self.walk.locate(result)?);
                    }
                    self.walk.pop(previous);
                    variants.push(StateVariant { name, fields });
                }
                sort_names(self.cx, &mut variants, |variant| &variant.name, self.walk)?;
                Ok(StateShape::Enum(variants))
            }
            _ => unreachable!("only nominal declarations are queued"),
        }
    }
}

pub(super) fn charge_fields(count: usize, walk: &mut Walk) -> SnapshotResult<()> {
    // ObjectType::field and enum member selection are linear. Names have already
    // passed identifier admission; conservatively account for every comparison.
    walk.budget
        .work((count as u64).saturating_mul(walk.budget.limits.max_identifier_bytes as u64))
}

fn sort_names<T>(
    cx: &ConversionContext<'_>,
    values: &mut [T],
    name: impl Fn(&T) -> &str,
    walk: &mut Walk,
) -> SnapshotResult<()> {
    for index in 1..values.len() {
        let mut slot = index;
        while slot > 0
            && walk
                .compare_text(cx, name(&values[slot - 1]), name(&values[slot]))?
                .is_gt()
        {
            values.swap(slot - 1, slot);
            slot -= 1;
        }
    }
    Ok(())
}

pub(super) fn copy_key(
    cx: &ConversionContext<'_>,
    key: &NominalKey,
    walk: &mut Walk,
) -> SnapshotResult<NominalKey> {
    let mut module = walk.output(key.module.len(), true)?;
    for name in &key.module {
        module.push(walk.copy_text(cx, name, true)?);
    }
    let mut declaration = walk.output(key.declaration.len(), true)?;
    for name in &key.declaration {
        declaration.push(walk.copy_text(cx, name, true)?);
    }
    let mut arguments = walk.output(key.arguments.len(), true)?;
    for argument in &key.arguments {
        arguments.push(copy_type(cx, argument, walk)?);
    }
    Ok(NominalKey {
        package: walk.copy_text(cx, &key.package, true)?,
        module,
        declaration,
        arguments,
    })
}

fn copy_type(
    cx: &ConversionContext<'_>,
    ty: &StateType,
    walk: &mut Walk,
) -> SnapshotResult<StateType> {
    walk.poll(cx)?;
    walk.budget.work(1)?;
    Ok(match ty {
        StateType::Scalar(scalar) => StateType::Scalar(*scalar),
        StateType::Sequence(element) => {
            StateType::Sequence(Box::new(copy_type(cx, element, walk)?))
        }
        StateType::Tuple(fields) => {
            let mut result = walk.output(fields.len(), true)?;
            for field in fields {
                result.push(copy_type(cx, field, walk)?);
            }
            StateType::Tuple(result)
        }
        StateType::Nominal(key) => StateType::Nominal(copy_key(cx, key, walk)?),
    })
}

pub(super) fn lookup<'a>(
    cx: &ConversionContext<'_>,
    schema: &'a StateSchema,
    key: &NominalKey,
    walk: &mut Walk,
) -> SnapshotResult<&'a StateDefinition> {
    let (mut start, mut end) = (0, schema.definitions.len());
    while start < end {
        let middle = start + (end - start) / 2;
        match compare::key(cx, key, &schema.definitions[middle].key, walk)? {
            Ordering::Equal => return Ok(&schema.definitions[middle]),
            Ordering::Less => end = middle,
            Ordering::Greater => start = middle + 1,
        }
    }
    Err(walk.error(
        SnapshotErrorKind::InvalidSchema,
        "missing nominal definition",
    ))
}

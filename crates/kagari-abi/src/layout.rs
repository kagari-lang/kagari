//! Nominal aggregate layouts used to verify field operands before bytecode emission.

use kagari_common::identity::reference::DefinitionReference;
mod applications;

use crate::{
    standard::surface::StandardEnum as StandardEnumKind,
    types::{AbiType, PublicAbiItem, TypeAbi, TypeAbiKind, substitution::TypeSubstitution},
};

use kagari_common::{
    cancellation::{CancellationToken, Cancelled},
    identity::{DefinitionKind, DefinitionPath, ModuleIdentity},
};

use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + serde::Serialize",
    deserialize = "I: DefinitionReference + serde::Deserialize<'de>"
))]
pub struct EnumLayout<I = DefinitionPath> {
    pub declaration: I,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub arguments: Vec<AbiType<I>>,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub variants: Vec<EnumVariantLayout<I>>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + serde::Serialize",
    deserialize = "I: DefinitionReference + serde::Deserialize<'de>"
))]
pub struct EnumVariantLayout<I = DefinitionPath> {
    pub declaration: I,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub payload: Vec<AbiType<I>>,
}

/// Compare executable instances to validated public declarations after substitution.
pub fn struct_abi_matches(
    layouts: &[StructLayout],
    identity: &ModuleIdentity,
    items: &[PublicAbiItem],
    cancel: &CancellationToken,
) -> Result<bool, Cancelled> {
    let templates = public_templates(items, TypeAbiKind::Struct, cancel)?;
    for layout in layouts {
        cancel.check()?;
        if &layout.declaration.module != identity {
            continue;
        }
        let Some(template) = layout
            .declaration
            .path
            .last()
            .and_then(|part| templates.get(part.name.as_str()))
        else {
            continue;
        };
        if layout.arguments.len() != template.generic_params.len()
            || layout.fields.len() != template.fields.len()
        {
            return Ok(false);
        }
        for (field, abi) in layout.fields.iter().zip(&template.fields) {
            cancel.check()?;
            if field.name != abi.name
                || field.mutable != abi.mutable
                || TypeSubstitution::for_owner(&layout.declaration, &layout.arguments)
                    .apply(&abi.ty, cancel)
                    .ok()
                    .as_ref()
                    != Some(&field.ty)
            {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

pub fn enum_abi_matches(
    layouts: &[EnumLayout],
    identity: &ModuleIdentity,
    items: &[PublicAbiItem],
    cancel: &CancellationToken,
) -> Result<bool, Cancelled> {
    let templates = public_templates(items, TypeAbiKind::Enum, cancel)?;
    let mut instantiated = HashSet::new();
    for layout in layouts {
        cancel.check()?;
        if &layout.declaration.module != identity {
            continue;
        }
        let Some(template) = layout
            .declaration
            .path
            .last()
            .and_then(|part| templates.get(part.name.as_str()))
        else {
            continue;
        };
        instantiated.insert(template.name.as_str());
        if layout.arguments.len() != template.generic_params.len()
            || layout.variants.len() != template.variants.len()
        {
            return Ok(false);
        }
        for (variant, abi) in layout.variants.iter().zip(&template.variants) {
            cancel.check()?;
            if !variant
                .declaration
                .path
                .last()
                .is_some_and(|part| part.name == abi.name)
                || variant.payload.len() != abi.payload.len()
            {
                return Ok(false);
            }
            for (concrete, ty) in variant.payload.iter().zip(&abi.payload) {
                cancel.check()?;
                if TypeSubstitution::for_owner(&layout.declaration, &layout.arguments)
                    .apply(ty, cancel)
                    .ok()
                    .as_ref()
                    != Some(concrete)
                {
                    return Ok(false);
                }
            }
        }
    }
    for template in templates.values() {
        cancel.check()?;
        if template.generic_params.is_empty() && !instantiated.contains(template.name.as_str()) {
            return Ok(false);
        }
    }
    Ok(true)
}

fn public_templates<'a>(
    items: &'a [PublicAbiItem],
    kind: TypeAbiKind,
    cancel: &CancellationToken,
) -> Result<HashMap<&'a str, &'a TypeAbi>, Cancelled> {
    cancel.check()?;
    let mut templates = HashMap::new();
    for item in items {
        cancel.check()?;
        if let PublicAbiItem::Type(ty) = item
            && ty.kind == kind
        {
            templates.insert(ty.name.as_str(), ty);
        }
    }
    Ok(templates)
}

pub fn validate_enum_layouts(
    layouts: &[EnumLayout],
    structures: &[StructLayout],
    cancel: &CancellationToken,
) -> Result<(), LayoutValidationError> {
    cancel
        .check()
        .map_err(|_| LayoutValidationError::Cancelled)?;
    if structures.iter().any(|layout| !layout.types_valid(cancel))
        || layouts.iter().any(|layout| !layout.types_valid(cancel))
    {
        cancel
            .check()
            .map_err(|_| LayoutValidationError::Cancelled)?;
        return Err(LayoutValidationError::Invalid);
    }
    let structure_templates: HashMap<_, _> = structures
        .iter()
        .filter(|layout| layout.arguments.iter().any(|ty| !ty.is_concrete()))
        .map(|layout| (&layout.declaration, layout))
        .collect();
    for applied in structures {
        cancel
            .check()
            .map_err(|_| LayoutValidationError::Cancelled)?;
        if let Some(template) = structure_templates.get(&applied.declaration)
            && template.apply(&applied.arguments, cancel).as_deref() != Some(applied)
        {
            return Err(LayoutValidationError::Invalid);
        }
    }
    let enum_templates: HashMap<_, _> = layouts
        .iter()
        .filter(|layout| layout.arguments.iter().any(|ty| !ty.is_concrete()))
        .map(|layout| (&layout.declaration, layout))
        .collect();
    for applied in layouts {
        cancel
            .check()
            .map_err(|_| LayoutValidationError::Cancelled)?;
        if let Some(template) = enum_templates.get(&applied.declaration)
            && template.apply(&applied.arguments, cancel).as_deref() != Some(applied)
        {
            return Err(LayoutValidationError::Invalid);
        }
    }
    let mut pending = structures
        .iter()
        .flat_map(|s| {
            s.arguments
                .iter()
                .chain(s.fields.iter().map(|field| &field.ty))
        })
        .chain(layouts.iter().flat_map(|e| &e.arguments))
        .collect::<Vec<_>>();
    let mut identities = HashSet::new();
    for layout in layouts {
        cancel
            .check()
            .map_err(|_| LayoutValidationError::Cancelled)?;
        let id = &layout.declaration;
        if id.path.len() != 1
            || id.module.package.0.is_empty()
            || id.module.path.is_empty()
            || id.module.path.iter().any(String::is_empty)
            || !id.path.last().is_some_and(|p| {
                p.kind == DefinitionKind::Enum && p.occurrence == 0 && !p.name.is_empty()
            })
            || !identities.insert((id, &layout.arguments))
        {
            return Err(LayoutValidationError::Invalid);
        }
        let mut names = HashSet::new();
        if layout.variants.len() > u32::MAX as usize || layouts.len() > u32::MAX as usize {
            return Err(LayoutValidationError::Limit {
                resource: "enum layout slots",
                limit: u32::MAX as usize,
            });
        }
        for variant in &layout.variants {
            cancel
                .check()
                .map_err(|_| LayoutValidationError::Cancelled)?;
            let child = &variant.declaration;
            if child.module != id.module
                || child.path.len() != id.path.len() + 1
                || !child.path.starts_with(&id.path)
                || !child.path.last().is_some_and(|p| {
                    p.kind == DefinitionKind::Variant
                        && p.occurrence == 0
                        && !p.name.is_empty()
                        && names.insert(&p.name)
                })
            {
                return Err(LayoutValidationError::Invalid);
            }
            pending.extend(&variant.payload);
        }
    }
    while let Some(ty) = pending.pop() {
        cancel
            .check()
            .map_err(|_| LayoutValidationError::Cancelled)?;

        match ty {
            AbiType::Projection { .. } | AbiType::SelfType(_) => {
                return Err(LayoutValidationError::Invalid);
            }
            AbiType::Builtin(_) | AbiType::Host(_) | AbiType::Parameter { .. } => {}
            AbiType::Tuple(types) => pending.extend(types),
            AbiType::Function { params, result } => {
                pending.extend(params);
                pending.push(result);
            }
            AbiType::Array(ty, _)
            | AbiType::Set(ty, _)
            | AbiType::Iter(ty)
            | AbiType::Range(ty, _) => pending.push(ty),
            AbiType::Map { key, value, .. } => {
                pending.push(key);
                pending.push(value);
            }
            AbiType::StandardEnum { kind, args } => {
                let expected = match kind {
                    StandardEnumKind::Ordering
                    | StandardEnumKind::ParseError
                    | StandardEnumKind::TryFromIntError
                    | StandardEnumKind::Infallible => 0,
                    StandardEnumKind::Bound | StandardEnumKind::Option => 1,
                    StandardEnumKind::Result => 2,
                };
                if args.len() != expected {
                    return Err(LayoutValidationError::Invalid);
                }
                pending.extend(args);
            }
            AbiType::Struct(instance)
            | AbiType::NativeObject(instance)
            | AbiType::Enum(instance)
            | AbiType::Trait(instance) => {
                pending.extend(&instance.arguments);
                pending.extend(instance.associated_types.values());
                let id = &instance.declaration;
                let kind = match ty {
                    AbiType::Struct(_) => DefinitionKind::Struct,
                    AbiType::NativeObject(_) => DefinitionKind::AssociatedType,
                    AbiType::Enum(_) => DefinitionKind::Enum,
                    _ => DefinitionKind::Trait,
                };
                if id.path.len() != 1
                    || id.module.package.0.is_empty()
                    || id.module.path.is_empty()
                    || id.module.path.iter().any(String::is_empty)
                    || !id
                        .path
                        .last()
                        .is_some_and(|p| p.kind == kind && !p.name.is_empty())
                {
                    return Err(LayoutValidationError::Invalid);
                }
                if (kind == DefinitionKind::Struct
                    && !structures.iter().any(|layout| {
                        &layout.declaration == id && layout.accepts(&instance.arguments)
                    }))
                    || (kind == DefinitionKind::Enum
                        && !layouts.iter().any(|layout| {
                            &layout.declaration == id && layout.accepts(&instance.arguments)
                        }))
                {
                    return Err(LayoutValidationError::Invalid);
                }
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + serde::Serialize",
    deserialize = "I: DefinitionReference + serde::Deserialize<'de>"
))]
pub struct StructLayout<I = DefinitionPath> {
    pub declaration: I,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub arguments: Vec<AbiType<I>>,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub fields: Vec<StructFieldLayout<I>>,
}

impl StructLayout {
    pub fn name(&self) -> &str {
        self.declaration
            .path
            .last()
            .map(|part| part.name.as_str())
            .unwrap_or("")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + serde::Serialize",
    deserialize = "I: DefinitionReference + serde::Deserialize<'de>"
))]
pub struct StructFieldLayout<I = DefinitionPath> {
    pub declaration: I,
    pub name: String,
    pub ty: AbiType<I>,
    pub mutable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayoutValidationError {
    Invalid,
    Limit {
        resource: &'static str,
        limit: usize,
    },
    Cancelled,
}

pub fn validate_layouts(
    layouts: &[StructLayout],
    cancel: &CancellationToken,
) -> Result<(), LayoutValidationError> {
    let limit = |count: usize, resource| {
        if count <= u32::MAX as usize {
            Ok(())
        } else {
            Err(LayoutValidationError::Limit {
                resource,
                limit: u32::MAX as usize,
            })
        }
    };
    cancel
        .check()
        .map_err(|_| LayoutValidationError::Cancelled)?;
    limit(layouts.len(), "struct layouts")?;
    let mut identities = HashSet::new();
    let mut field_count = 0usize;
    for structure in layouts {
        cancel
            .check()
            .map_err(|_| LayoutValidationError::Cancelled)?;
        let id = &structure.declaration;
        if id.path.len() != 1
            || id.module.package.0.is_empty()
            || id.module.path.is_empty()
            || id.module.path.iter().any(String::is_empty)
            || !id.path.last().is_some_and(|part| {
                part.kind == DefinitionKind::Struct && part.occurrence == 0 && !part.name.is_empty()
            })
            || !identities.insert((id, &structure.arguments))
        {
            return Err(LayoutValidationError::Invalid);
        }
        field_count = field_count
            .checked_add(structure.fields.len())
            .ok_or(LayoutValidationError::Invalid)?;
        limit(field_count, "struct fields")?;
        let mut names = HashSet::new();
        for field in &structure.fields {
            cancel
                .check()
                .map_err(|_| LayoutValidationError::Cancelled)?;
            let field_id = &field.declaration;
            if field_id.module != id.module
                || field_id.path.len() != id.path.len() + 1
                || !field_id.path.starts_with(&id.path)
                || !field_id.path.last().is_some_and(|part| {
                    part.kind == DefinitionKind::Field
                        && part.occurrence == 0
                        && part.name == field.name
                })
                || field.name.is_empty()
                || !names.insert(&field.name)
            {
                return Err(LayoutValidationError::Invalid);
            }
        }
    }
    Ok(())
}

mod mapping;

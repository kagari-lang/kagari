//! Nominal aggregate layouts used to verify field operands before bytecode emission.
use kagari_common::identity::DefinitionId;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EnumLayout {
    pub declaration: DefinitionId,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub arguments: Vec<super::abi::AbiType>,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub variants: Vec<EnumVariantLayout>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EnumVariantLayout {
    pub declaration: DefinitionId,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub payload: Vec<super::abi::AbiType>,
}

/// Compare executable instances to validated public declarations after substitution.
pub(crate) fn struct_abi_matches(
    layouts: &[StructLayout],
    identity: &kagari_common::identity::ModuleIdentity,
    items: &[super::PublicAbiItem],
    cancel: &kagari_common::cancellation::CancellationToken,
) -> Result<bool, kagari_common::cancellation::Cancelled> {
    let templates = public_templates(items, super::TypeAbiKind::Struct, cancel)?;
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
                || abi
                    .ty
                    .instantiate(&layout.declaration, &layout.arguments)
                    .as_ref()
                    != Some(&field.ty)
            {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

pub(crate) fn enum_abi_matches(
    layouts: &[EnumLayout],
    identity: &kagari_common::identity::ModuleIdentity,
    items: &[super::PublicAbiItem],
    cancel: &kagari_common::cancellation::CancellationToken,
) -> Result<bool, kagari_common::cancellation::Cancelled> {
    let templates = public_templates(items, super::TypeAbiKind::Enum, cancel)?;
    let mut instantiated = std::collections::HashSet::new();
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
                if ty
                    .instantiate(&layout.declaration, &layout.arguments)
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
    items: &'a [super::PublicAbiItem],
    kind: super::TypeAbiKind,
    cancel: &kagari_common::cancellation::CancellationToken,
) -> Result<
    std::collections::HashMap<&'a str, &'a super::TypeAbi>,
    kagari_common::cancellation::Cancelled,
> {
    cancel.check()?;
    let mut templates = std::collections::HashMap::new();
    for item in items {
        cancel.check()?;
        if let super::PublicAbiItem::Type(ty) = item
            && ty.kind == kind
        {
            templates.insert(ty.name.as_str(), ty);
        }
    }
    Ok(templates)
}

pub(crate) fn validate_enum_layouts(
    layouts: &[EnumLayout],
    structures: &[StructLayout],
    cancel: &kagari_common::cancellation::CancellationToken,
) -> Result<(), LayoutValidationError> {
    use kagari_common::identity::DefinitionKind;
    let mut pending = structures
        .iter()
        .flat_map(|s| {
            s.arguments
                .iter()
                .chain(s.fields.iter().map(|field| &field.ty))
        })
        .chain(layouts.iter().flat_map(|e| &e.arguments))
        .collect::<Vec<_>>();
    let mut identities = std::collections::HashSet::new();
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
        let mut names = std::collections::HashSet::new();
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
        use super::abi::{AbiType, StandardEnumKind};
        match ty {
            AbiType::Parameter { .. } | AbiType::SelfType(_) => {
                return Err(LayoutValidationError::Invalid);
            }
            AbiType::Builtin(_) | AbiType::Host(_) => {}
            AbiType::Tuple(types) => pending.extend(types),
            AbiType::Array(ty) | AbiType::Set(ty) => pending.push(ty),
            AbiType::Map { key, value } => {
                pending.push(key);
                pending.push(value);
            }
            AbiType::StandardEnum { kind, args } => {
                let expected = match kind {
                    StandardEnumKind::Option => 1,
                    StandardEnumKind::Result => 2,
                };
                if args.len() != expected {
                    return Err(LayoutValidationError::Invalid);
                }
                pending.extend(args);
            }
            AbiType::Struct(instance) | AbiType::Enum(instance) | AbiType::Trait(instance) => {
                pending.extend(&instance.arguments);
                let id = &instance.declaration;
                let kind = match ty {
                    AbiType::Struct(_) => DefinitionKind::Struct,
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
                        &layout.declaration == id && layout.arguments == instance.arguments
                    }))
                    || (kind == DefinitionKind::Enum
                        && !layouts.iter().any(|layout| {
                            &layout.declaration == id && layout.arguments == instance.arguments
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
pub struct StructLayout {
    pub declaration: DefinitionId,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub arguments: Vec<super::abi::AbiType>,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub fields: Vec<StructFieldLayout>,
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
pub struct StructFieldLayout {
    pub declaration: DefinitionId,
    pub name: String,
    pub ty: super::abi::AbiType,
    pub mutable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LayoutValidationError {
    Invalid,
    Limit {
        resource: &'static str,
        limit: usize,
    },
    Cancelled,
}
pub(crate) fn validate_layouts(
    layouts: &[StructLayout],
    cancel: &kagari_common::cancellation::CancellationToken,
) -> Result<(), LayoutValidationError> {
    use kagari_common::identity::DefinitionKind;
    use std::collections::HashSet;
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

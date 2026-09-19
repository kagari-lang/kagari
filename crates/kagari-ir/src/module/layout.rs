//! Nominal aggregate layouts used to verify field operands before bytecode emission.
use super::ValueType;
use kagari_common::identity::DefinitionId;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EnumLayout {
    pub declaration: DefinitionId,
    pub variants: Vec<EnumVariantLayout>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EnumVariantLayout {
    pub declaration: DefinitionId,
    pub payload: Vec<super::abi::AbiType>,
}

pub(crate) fn enum_abi_matches(
    layouts: &[EnumLayout],
    identity: &kagari_common::identity::ModuleIdentity,
    items: &[super::PublicAbiItem],
) -> bool {
    items.iter().all(|item| {
        let super::PublicAbiItem::Type(ty) = item else {
            return true;
        };
        if ty.kind != super::TypeAbiKind::Enum {
            return true;
        }
        ty.fields.is_empty()
            && layouts
                .iter()
                .find(|layout| {
                    &layout.declaration.module == identity
                        && layout
                            .declaration
                            .path
                            .last()
                            .is_some_and(|part| part.name == ty.name)
                })
                .is_some_and(|layout| {
                    layout.variants.len() == ty.variants.len()
                        && layout
                            .variants
                            .iter()
                            .zip(&ty.variants)
                            .all(|(layout, abi)| {
                                layout
                                    .declaration
                                    .path
                                    .last()
                                    .is_some_and(|part| part.name == abi.name)
                                    && layout.payload == abi.payload
                            })
                })
    })
}

pub(crate) fn validate_enum_layouts(
    layouts: &[EnumLayout],
    structures: &[StructLayout],
    cancel: &kagari_common::cancellation::CancellationToken,
) -> Result<(), LayoutValidationError> {
    use kagari_common::identity::DefinitionKind;
    let mut identities = std::collections::HashSet::new();
    for layout in layouts {
        cancel
            .check()
            .map_err(|_| LayoutValidationError::Cancelled)?;
        let id = &layout.declaration;
        if id.module.package.0.is_empty()
            || id.module.path.is_empty()
            || id.module.path.iter().any(String::is_empty)
            || !id
                .path
                .last()
                .is_some_and(|p| p.kind == DefinitionKind::Enum && !p.name.is_empty())
            || !identities.insert(id)
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
                    p.kind == DefinitionKind::Variant && !p.name.is_empty() && names.insert(&p.name)
                })
            {
                return Err(LayoutValidationError::Invalid);
            }
            let mut pending = variant.payload.iter().collect::<Vec<_>>();
            while let Some(ty) = pending.pop() {
                cancel
                    .check()
                    .map_err(|_| LayoutValidationError::Cancelled)?;
                use super::abi::{AbiType, StandardEnumKind};
                match ty {
                    AbiType::Builtin(_) => {}
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
                    AbiType::Struct(id) | AbiType::Enum(id) | AbiType::Trait(id) => {
                        let kind = match ty {
                            AbiType::Struct(_) => DefinitionKind::Struct,
                            AbiType::Enum(_) => DefinitionKind::Enum,
                            _ => DefinitionKind::Trait,
                        };
                        if id.module.package.0.is_empty()
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
                            && !structures.iter().any(|layout| &layout.declaration == id))
                            || (kind == DefinitionKind::Enum
                                && !layouts.iter().any(|layout| &layout.declaration == id))
                        {
                            return Err(LayoutValidationError::Invalid);
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StructLayout {
    pub declaration: DefinitionId,
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
    pub ty: ValueType,
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
        if id.module.package.0.is_empty()
            || id.module.path.is_empty()
            || id.module.path.iter().any(String::is_empty)
            || !id
                .path
                .last()
                .is_some_and(|part| part.kind == DefinitionKind::Struct && !part.name.is_empty())
            || !identities.insert(id)
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
                    part.kind == DefinitionKind::Field && part.name == field.name
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

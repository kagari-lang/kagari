//! Nominal aggregate layouts used to verify field operands before bytecode emission.
use kagari_common::identity::DefinitionId;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EnumLayout {
    pub declaration: DefinitionId,
    pub arguments: Vec<super::abi::AbiType>,
    pub variants: Vec<EnumVariantLayout>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EnumVariantLayout {
    pub declaration: DefinitionId,
    pub payload: Vec<super::abi::AbiType>,
}

/// Compare executable instances to the owning public declaration after substitution.
pub(crate) fn struct_abi_matches(
    layouts: &[StructLayout],
    identity: &kagari_common::identity::ModuleIdentity,
    items: &[super::PublicAbiItem],
) -> bool {
    items.iter().all(|item| {
        let super::PublicAbiItem::Type(ty) = item else {
            return true;
        };
        if ty.kind != super::TypeAbiKind::Struct {
            return true;
        }
        layouts
            .iter()
            .filter(|layout| {
                &layout.declaration.module == identity
                    && layout
                        .declaration
                        .path
                        .last()
                        .is_some_and(|part| part.name == ty.name)
            })
            .all(|layout| {
                layout.arguments.len() == ty.generic_params.len()
                    && layout.fields.len() == ty.fields.len()
                    && layout.fields.iter().zip(&ty.fields).all(|(field, abi)| {
                        field.name == abi.name
                            && field.mutable == abi.mutable
                            && abi
                                .ty
                                .instantiate(&layout.declaration, &layout.arguments)
                                .as_ref()
                                == Some(&field.ty)
                    })
            })
    })
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
        let instances = layouts
            .iter()
            .filter(|layout| {
                &layout.declaration.module == identity
                    && layout
                        .declaration
                        .path
                        .last()
                        .is_some_and(|part| part.name == ty.name)
            })
            .collect::<Vec<_>>();
        (!ty.generic_params.is_empty() || !instances.is_empty())
            && instances.into_iter().all(|layout| {
                layout.arguments.len() == ty.generic_params.len()
                    && layout.variants.len() == ty.variants.len()
                    && layout
                        .variants
                        .iter()
                        .zip(&ty.variants)
                        .all(|(variant, abi)| {
                            variant
                                .declaration
                                .path
                                .last()
                                .is_some_and(|part| part.name == abi.name)
                                && abi
                                    .payload
                                    .iter()
                                    .map(|ty| {
                                        ty.instantiate(&layout.declaration, &layout.arguments)
                                    })
                                    .collect::<Option<Vec<_>>>()
                                    == Some(variant.payload.clone())
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
    pub arguments: Vec<super::abi::AbiType>,
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

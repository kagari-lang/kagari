//! Nominal aggregate layouts used to verify field operands before bytecode emission.

use crate::types::PublicItem;
use kagari_common::{
    cancellation::{CancellationToken, Cancelled},
    identity::{
        DefinitionKind, ModuleIdentity,
        table::{DefinitionId, DefinitionTable},
    },
};
use kagari_types::{
    declaration::{TypeDef, TypeDefKind, verify::DeclarationValidationError},
    ty::{Ty, substitution::TypeSubstitution},
    visibility::Visibility,
};
use std::collections::{HashMap, HashSet};
mod applications;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EnumLayout {
    pub declaration: DefinitionId,
    pub arguments: Vec<Ty<DefinitionId>>,
    pub variants: Vec<EnumVariantLayout>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EnumVariantLayout {
    pub reports_failure: bool,
    pub declaration: DefinitionId,
    pub payload: Vec<Ty<DefinitionId>>,
}

/// Compare executable instances to validated public declarations after substitution.
pub fn struct_abi_matches(
    layouts: &[StructLayout],
    identity: &ModuleIdentity,
    items: &[PublicItem],
    cancel: &CancellationToken,
    definitions: &DefinitionTable,
) -> Result<bool, Cancelled> {
    let templates = public_templates(items, TypeDefKind::Struct, cancel)?;
    for layout in layouts {
        cancel.check()?;
        let Ok(declaration) = definitions.resolve(layout.declaration) else {
            return Ok(false);
        };
        if declaration.module() != identity {
            continue;
        }
        let Some(template) = declaration.last().and_then(|part| templates.get(part.name)) else {
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
                || field.visibility != abi.visibility
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
    items: &[PublicItem],
    cancel: &CancellationToken,
    definitions: &DefinitionTable,
) -> Result<bool, Cancelled> {
    let templates = public_templates(items, TypeDefKind::Enum, cancel)?;
    let mut instantiated = HashSet::new();
    for layout in layouts {
        cancel.check()?;
        let Ok(declaration) = definitions.resolve(layout.declaration) else {
            return Ok(false);
        };
        if declaration.module() != identity {
            continue;
        }
        let Some(template) = declaration.last().and_then(|part| templates.get(part.name)) else {
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
            if !definitions
                .resolve(variant.declaration)
                .ok()
                .and_then(|view| view.last())
                .is_some_and(|part| part.name == abi.name)
                || variant.payload.len() != abi.payload.len()
                || variant.reports_failure != abi.reports_failure
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
    items: &'a [PublicItem],
    kind: TypeDefKind,
    cancel: &CancellationToken,
) -> Result<HashMap<&'a str, &'a TypeDef<DefinitionId>>, Cancelled> {
    cancel.check()?;
    let mut templates = HashMap::new();
    for item in items {
        cancel.check()?;
        if let PublicItem::Type(ty) = item
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
    definitions: &DefinitionTable,
) -> Result<(), LayoutValidationError> {
    cancel
        .check()
        .map_err(|_| LayoutValidationError::Cancelled)?;
    if structures
        .iter()
        .any(|layout| !layout.types_valid(cancel, definitions))
        || layouts
            .iter()
            .any(|layout| !layout.types_valid(cancel, definitions))
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
        if !top_level(*id, DefinitionKind::Enum, Some(0), definitions)
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
            if definitions.parent(*child) != Ok(Some(*id))
                || !definitions
                    .resolve(*child)
                    .ok()
                    .and_then(|view| view.last())
                    .is_some_and(|p| {
                        p.kind == DefinitionKind::Variant
                            && p.occurrence == 0
                            && !p.name.is_empty()
                            && names.insert(p.name)
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
            Ty::Projection { .. } | Ty::SelfType(_) => {
                return Err(LayoutValidationError::Invalid);
            }
            Ty::Builtin(_) | Ty::Host(_) | Ty::Parameter { .. } => {}
            Ty::Tuple(types) => pending.extend(types),
            Ty::Function { params, result } => {
                pending.extend(params);
                pending.push(result);
            }
            Ty::Array(ty) | Ty::Set(ty, _) | Ty::Iter(ty) | Ty::Range(ty, _) => pending.push(ty),
            Ty::Map { key, value, .. } => {
                pending.push(key);
                pending.push(value);
            }

            Ty::Struct(instance)
            | Ty::NativeObject(instance)
            | Ty::Enum(instance)
            | Ty::Trait(instance) => {
                pending.extend(&instance.arguments);
                pending.extend(instance.associated_types.values());
                let id = &instance.declaration;
                let kind = match ty {
                    Ty::Struct(_) => DefinitionKind::Struct,
                    Ty::NativeObject(_) => DefinitionKind::AssociatedType,
                    Ty::Enum(_) => DefinitionKind::Enum,
                    _ => DefinitionKind::Trait,
                };
                if !top_level(*id, kind, None, definitions) {
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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StructLayout {
    pub declaration: DefinitionId,
    pub arguments: Vec<Ty<DefinitionId>>,
    pub fields: Vec<StructFieldLayout>,
}

impl StructLayout {
    pub fn name<'a>(&self, definitions: &'a DefinitionTable) -> Option<&'a str> {
        definitions
            .resolve(self.declaration)
            .ok()?
            .last()
            .map(|part| part.name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StructFieldLayout {
    pub visibility: Visibility,
    pub declaration: DefinitionId,
    pub name: String,
    pub ty: Ty<DefinitionId>,
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
    definitions: &DefinitionTable,
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
        if !top_level(*id, DefinitionKind::Struct, Some(0), definitions)
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
            if definitions.parent(*field_id) != Ok(Some(*id))
                || !definitions
                    .resolve(*field_id)
                    .ok()
                    .and_then(|view| view.last())
                    .is_some_and(|part| {
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

fn top_level(
    id: DefinitionId,
    kind: DefinitionKind,
    occurrence: Option<u32>,
    definitions: &DefinitionTable,
) -> bool {
    definitions.resolve(id).is_ok_and(|view| {
        view.segments().count() == 1
            && !view.module().package.0.is_empty()
            && !view.module().path.is_empty()
            && !view.module().path.iter().any(String::is_empty)
            && view.last().is_some_and(|part| {
                part.kind == kind
                    && occurrence.is_none_or(|value| part.occurrence == value)
                    && !part.name.is_empty()
            })
    })
}

mod mapping;

impl From<DeclarationValidationError> for LayoutValidationError {
    fn from(error: DeclarationValidationError) -> Self {
        match error {
            DeclarationValidationError::Invalid => Self::Invalid,
            DeclarationValidationError::Cancelled => Self::Cancelled,
        }
    }
}

#[doc(hidden)]
pub mod wire;

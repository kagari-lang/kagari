//! Nominal aggregate layouts used to verify field operands before bytecode emission.
use super::ValueType;
use kagari_common::identity::DefinitionId;

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

use super::{Context, IrVerificationError, IrVerificationErrorKind as Error};
use crate::module::IrModule;
use kagari_common::identity::DefinitionKind;
use std::collections::HashSet;

pub(super) fn verify(module: &IrModule, context: Context<'_>) -> Result<(), IrVerificationError> {
    context.limit(module.structures.len(), u32::MAX as usize, "struct layouts")?;
    let mut identities = HashSet::new();
    let mut field_count = 0usize;
    for structure in &module.structures {
        context.check_cancel()?;
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
            return Err(context.error(Error::InvalidStructLayout));
        }
        field_count = field_count
            .checked_add(structure.fields.len())
            .ok_or_else(|| context.error(Error::InvalidStructLayout))?;
        context.limit(field_count, u32::MAX as usize, "struct fields")?;
        let mut names = HashSet::new();
        for field in &structure.fields {
            context.check_cancel()?;
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
                return Err(context.error(Error::InvalidStructLayout));
            }
        }
    }
    Ok(())
}

//! Identity registration for the transient foundation assembly.
use crate::catalog::assembly_identity;
use kagari_common::identity::{DefinitionKind, map::DefinitionContext, table::DefinitionId};
use kagari_types::declaration::module::DeclarationError;
use std::fmt;

pub(super) fn invalid(error: impl fmt::Display) -> DeclarationError {
    DeclarationError(format!("foundation identity: {error}"))
}

pub(super) fn definition(
    context: &DefinitionContext,
    kind: DefinitionKind,
    name: &str,
) -> Result<DefinitionId, DeclarationError> {
    let root = context.intern_root(&assembly_identity()).map_err(invalid)?;
    context.intern_child(root, kind, name, 0).map_err(invalid)
}

pub(super) fn implementation(
    context: &DefinitionContext,
    index: usize,
) -> Result<DefinitionId, DeclarationError> {
    let root = context.intern_root(&assembly_identity()).map_err(invalid)?;
    let occurrence = u32::try_from(index).map_err(invalid)?;
    context
        .intern_child(root, DefinitionKind::Impl, "", occurrence)
        .map_err(invalid)
}

pub(super) fn method(
    context: &DefinitionContext,
    owner: &DefinitionId,
    name: &str,
) -> Result<DefinitionId, DeclarationError> {
    context
        .intern_child(*owner, DefinitionKind::Method, name, 0)
        .map_err(invalid)
}

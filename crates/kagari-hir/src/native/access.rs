//! Select source paths from checked module records without inspecting host storage.
use crate::{
    declarations::Declarations,
    native::NativeTypeKind,
    types::{TypeId, semantic::raise_type},
};
use kagari_common::identity::{
    DefinitionKind,
    table::{DefinitionId, DefinitionTable},
};
use kagari_types::declaration::path::{
    HostPathContract, HostPathDeclaration, HostPathSegmentDeclaration,
};
use std::collections::HashSet;

/// Checked source operations used to select an installed external path contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SourcePathStep {
    Member(String),
    Index(TypeId),
}

fn select<'a>(
    paths: &'a [HostPathDeclaration],
    definitions: &DefinitionTable,
    root: DefinitionId,
    steps: &[SourcePathStep],
) -> Result<&'a HostPathDeclaration, &'static str> {
    let mut matches = paths.iter().filter(|path| {
        if path.root != root || path.segments.len() != steps.len() {
            return false;
        }
        let mut slots = HashSet::new();
        path.segments
            .iter()
            .zip(steps)
            .all(|(segment, source)| match (segment, source) {
                (HostPathSegmentDeclaration::Field(id), SourcePathStep::Member(name)) => {
                    definitions
                        .resolve(*id)
                        .ok()
                        .and_then(|view| view.last())
                        .is_some_and(|leaf| {
                            leaf.kind == DefinitionKind::Field && leaf.name == *name
                        })
                }
                (HostPathSegmentDeclaration::Virtual(step), SourcePathStep::Member(name)) => {
                    step.name == *name
                }
                (HostPathSegmentDeclaration::Index(index), SourcePathStep::Index(ty)) => {
                    slots.insert(index.slot) && raise_type(&index.index) == *ty
                }
                _ => false,
            })
    });
    let declaration = matches.next().ok_or("path has no declared host contract")?;
    if matches.next().is_some() {
        return Err("path has ambiguous host declarations");
    }
    Ok(declaration)
}

impl Declarations {
    pub(crate) fn source_host_path(
        &self,
        root: DefinitionId,
        steps: &[SourcePathStep],
    ) -> Result<(HostPathDeclaration, HostPathContract), &'static str> {
        let Some((paths, schemas)) = self.native_types().find_map(|kind| match kind {
            NativeTypeKind::Host {
                declaration,
                paths,
                schemas,
                ..
            } if *declaration == root => Some((paths, schemas)),
            _ => None,
        }) else {
            return Err("path has no declared host contract");
        };
        let declaration = select(paths, self.definitions(), root, steps)?;
        let contract = declaration
            .contract(schemas, self.definitions())
            .map_err(|_| "invalid declared host path")?;
        Ok((declaration.clone(), contract))
    }
}

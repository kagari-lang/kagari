//! Symbolic enum references required by the existing language protocol contracts.
//! These are declaration identities, not type shapes or runtime discriminants.
//! Consumers must resolve and validate the actual installed declarations/layouts.
use crate::ty::{NominalTy, Ty};
use kagari_common::identity::table::{DefinitionId, DefinitionSegment, DefinitionTable};
use kagari_common::identity::{
    DefinitionKind, DefinitionPath, DefinitionPathSegment, ModuleIdentity, PackageId,
};

const OPTION: (&str, &str) = ("option", "Option");
const RESULT: (&str, &str) = ("result", "Result");

/// Match an external symbolic query against an ID in its retained scope.
pub fn matches(id: &DefinitionId, expected: &DefinitionPath, table: &DefinitionTable) -> bool {
    table.lookup(expected) == Some(*id)
}

fn declaration(module: &str, name: &str) -> DefinitionPath {
    DefinitionPath {
        module: ModuleIdentity {
            package: PackageId("kagari-core".into()),
            path: vec![module.into()],
        },
        path: vec![DefinitionPathSegment {
            kind: DefinitionKind::Enum,
            name: name.into(),
            occurrence: 0,
        }],
    }
}

fn declaration_id(table: &DefinitionTable, module: &str, name: &str) -> Option<DefinitionId> {
    table.lookup_components(
        "kagari-core",
        [module],
        [DefinitionSegment {
            kind: DefinitionKind::Enum,
            name,
            occurrence: 0,
        }],
    )
}

fn applied(table: &DefinitionTable, module: &str, name: &str, arguments: Vec<Ty>) -> Option<Ty> {
    Some(Ty::Enum(NominalTy {
        declaration: declaration_id(table, module, name)?,
        arguments,
        associated_types: Default::default(),
    }))
}

pub fn option_declaration() -> DefinitionPath {
    declaration(OPTION.0, OPTION.1)
}

/// Resolve Option's symbolic identity without allocating an authoring path.
/// The returned ID is table-local; its presence does not prove provider installation.
pub fn option_declaration_id(table: &DefinitionTable) -> Option<DefinitionId> {
    declaration_id(table, OPTION.0, OPTION.1)
}

pub fn result_declaration() -> DefinitionPath {
    declaration(RESULT.0, RESULT.1)
}

/// Resolve Result's symbolic identity without allocating an authoring path.
/// The returned ID is table-local; its presence does not prove provider installation.
pub fn result_declaration_id(table: &DefinitionTable) -> Option<DefinitionId> {
    declaration_id(table, RESULT.0, RESULT.1)
}

pub fn ordering_declaration() -> DefinitionPath {
    declaration("cmp", "Ordering")
}

pub fn bound_declaration() -> DefinitionPath {
    declaration("ops", "Bound")
}

/// Iterator.next's declared result constructor in the supplied scope.
/// Returns None when the nominal core declaration has not been registered.
pub fn option(table: &DefinitionTable, item: Ty) -> Option<Ty> {
    applied(table, OPTION.0, OPTION.1, vec![item])
}

/// Checked conversion result constructor in the supplied scope.
/// Returns None when the nominal core declaration has not been registered.
pub fn result(table: &DefinitionTable, value: Ty, error: Ty) -> Option<Ty> {
    applied(table, RESULT.0, RESULT.1, vec![value, error])
}

pub fn control_flow_declaration() -> DefinitionPath {
    declaration("ops", "ControlFlow")
}

/// Core enum declarations with conditional member comparison and hashing.
/// User enum shapes never imply these protocols; explicit implementations win.
pub fn member_comparison(id: &DefinitionId, table: &DefinitionTable) -> bool {
    [
        option_declaration(),
        result_declaration(),
        ordering_declaration(),
        bound_declaration(),
        control_flow_declaration(),
    ]
    .iter()
    .any(|declaration| matches(id, declaration, table))
}

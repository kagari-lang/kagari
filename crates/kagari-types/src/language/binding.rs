//! Symbolic enum references required by the existing language protocol contracts.
//! These are declaration identities, not type shapes or runtime discriminants.
//! Consumers must resolve and validate the actual installed declarations/layouts.
use crate::ty::{NominalTy, Ty};
use kagari_common::identity::{
    DefinitionKind, DefinitionPath, DefinitionPathSegment, ModuleIdentity, PackageId,
};
use kagari_common::identity::{reference::DefinitionReference, table::DefinitionTable};

pub fn matches<I: DefinitionReference>(
    id: &I,
    expected: &DefinitionPath,
    table: Option<&DefinitionTable>,
) -> bool {
    id.describe(table).is_ok_and(|actual| {
        actual.module() == &expected.module
            && actual
                .segments()
                .map(|part| (part.kind, part.name, part.occurrence))
                .eq(expected
                    .path
                    .iter()
                    .map(|part| (part.kind, part.name.as_str(), part.occurrence)))
    })
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

fn applied(table: &DefinitionTable, module: &str, name: &str, arguments: Vec<Ty>) -> Option<Ty> {
    Some(Ty::Enum(NominalTy {
        declaration: table.lookup(&declaration(module, name))?,
        arguments,
        associated_types: Default::default(),
    }))
}

pub fn option_declaration() -> DefinitionPath {
    declaration("option", "Option")
}

pub fn result_declaration() -> DefinitionPath {
    declaration("result", "Result")
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
    applied(table, "option", "Option", vec![item])
}

/// Checked conversion result constructor in the supplied scope.
/// Returns None when the nominal core declaration has not been registered.
pub fn result(table: &DefinitionTable, value: Ty, error: Ty) -> Option<Ty> {
    applied(table, "result", "Result", vec![value, error])
}

pub fn control_flow_declaration() -> DefinitionPath {
    declaration("ops", "ControlFlow")
}

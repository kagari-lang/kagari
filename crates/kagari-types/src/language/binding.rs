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

fn applied(module: &str, name: &str, arguments: Vec<Ty>) -> Ty {
    Ty::Enum(NominalTy {
        declaration: declaration(module, name),
        arguments,
        associated_types: Default::default(),
    })
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

/// Iterator.next's declared result constructor.
pub fn option(item: Ty) -> Ty {
    applied("option", "Option", vec![item])
}

/// Checked conversion result constructor.
pub fn result(value: Ty, error: Ty) -> Ty {
    applied("result", "Result", vec![value, error])
}

/// Comparison protocols' declared result type.
pub fn ordering() -> Ty {
    applied("cmp", "Ordering", vec![])
}

/// RangeBounds' declared endpoint constructor.
pub fn bound(item: Ty) -> Ty {
    applied("ops", "Bound", vec![item])
}

/// Try.branch's ordinary result constructor.
pub fn control_flow(residual: Ty, output: Ty) -> Ty {
    applied("ops", "ControlFlow", vec![residual, output])
}

pub fn control_flow_declaration() -> DefinitionPath {
    declaration("ops", "ControlFlow")
}

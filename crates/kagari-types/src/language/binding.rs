//! Symbolic enum references required by the existing language protocol contracts.
//! These are declaration identities, not type shapes or runtime discriminants.
//! Consumers must resolve and validate the actual installed declarations/layouts.
use crate::{
    scalar::BuiltinType,
    ty::{NominalTy, Ty},
};
use kagari_common::identity::table::{DefinitionId, DefinitionTable};
use kagari_common::identity::{
    DefinitionKind, DefinitionPath, DefinitionPathSegment, ModuleIdentity, PackageId,
};

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

/// Branch keeps the error type when making a residual. FromResidual can change
/// the error type, but only when the input's success type is `!`.
pub fn result_origin_carriers_valid(
    original: &Ty,
    destination: &Ty,
    table: &DefinitionTable,
) -> bool {
    let (Ty::Enum(source), Ty::Enum(target)) = (original, destination) else {
        return false;
    };
    let ([success, error], [output, converted]) =
        (source.arguments.as_slice(), target.arguments.as_slice())
    else {
        return false;
    };
    let never = Ty::Builtin(BuiltinType::Never);
    source.declaration == target.declaration
        && source.associated_types.is_empty()
        && target.associated_types.is_empty()
        && matches(&source.declaration, &result_declaration(), table)
        && (*success == never || (*output == never && error == converted))
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

#[cfg(test)]
mod tests {
    use super::{declaration, result, result_declaration, result_origin_carriers_valid};
    use crate::{
        scalar::BuiltinType,
        ty::{NominalTy, Ty},
    };
    use kagari_common::identity::table::DefinitionTableBuilder;

    #[test]
    fn result_origin_transfer_rejects_nominal_markers_and_branch_error_changes() {
        let mut definitions = DefinitionTableBuilder::new().unwrap();
        definitions.intern_path(&result_declaration()).unwrap();
        let empty = definitions
            .intern_path(&declaration("test", "Empty"))
            .unwrap();
        let table = definitions.freeze();
        let carrier = |success, error| result(&table, success, Ty::Builtin(error)).unwrap();
        let empty = Ty::Enum(NominalTy {
            declaration: empty,
            arguments: vec![],
            associated_types: Default::default(),
        });
        let ordinary = carrier(Ty::Builtin(BuiltinType::Bool), BuiltinType::I32);
        let nominal = carrier(empty, BuiltinType::I32);
        let changed_residual = carrier(Ty::Builtin(BuiltinType::Never), BuiltinType::I64);

        assert!(!result_origin_carriers_valid(&nominal, &ordinary, &table));
        assert!(!result_origin_carriers_valid(&ordinary, &nominal, &table));
        assert!(!result_origin_carriers_valid(
            &ordinary,
            &changed_residual,
            &table,
        ));
    }
}

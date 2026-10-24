//! Export-only layout roots use the same source ownership and budgets as body roots.

use super::{checked, insert};
use crate::{
    bytecode::lower_program_to_bytecode,
    source::{
        lower::{MirLoweringError, instances::MirLoweringOptions},
        program::{SourceProgramError, lower_program_to_mir},
    },
};
use kagari_contract::exports::TypeExport;
use kagari_hir::{analysis::AnalysisDatabase, program::ProgramCheckError};
use kagari_mir::codec::{decode_program, encode_program};
use kagari_source::{diagnostic::DiagnosticKind, source_database::SourceDatabase};
use kagari_types::{declaration::TypeDefKind, scalar::BuiltinType, ty::Ty};
use rstest::rstest;

const BOUNDED_PRIVATE_APPLICATION: &str =
    include_str!("../../../tests/fixtures/type_exports/bounded-private-argument.kgr");

#[rstest]
#[case::private_argument(
    include_str!("../../../tests/fixtures/type_exports/private-argument.kgr"),
    false
)]
#[case::bounded_private_argument(BOUNDED_PRIVATE_APPLICATION, true)]
fn public_generic_layouts_keep_private_argument_contracts(
    #[case] source: &str,
    #[case] bounded: bool,
) {
    let mut sources = SourceDatabase::default();
    let root = insert(&mut sources, "root", source);
    let checked = AnalysisDatabase::default()
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap()
        .check_program(root, &Default::default())
        .unwrap();
    let program = lower_program_to_mir(&checked, &Default::default()).unwrap();
    let bytes = encode_program(&program, &Default::default()).unwrap();
    let decoded = decode_program(&bytes, &Default::default()).unwrap();
    let exports = &decoded.exports().namespaces[decoded.root()];
    assert!(matches!(exports.types["Cell"], TypeExport::Declaration(_)));
    for private in ["Hidden", "Nested", "Tag"] {
        assert!(!exports.types.contains_key(private));
        assert!(decoded.modules().iter().all(|module| {
            module
                .abi
                .public_items
                .iter()
                .all(|item| item.name() != private)
        }));
    }
    let owner = decoded
        .modules()
        .iter()
        .find(|module| &module.identity == decoded.root())
        .unwrap();
    let hidden = owner
        .abi
        .private_types
        .iter()
        .find(|record| record.definition.name == "Hidden")
        .unwrap();
    assert_eq!(hidden.definition.kind, TypeDefKind::Struct);
    assert_eq!(
        decoded
            .definitions()
            .resolve(hidden.declaration)
            .unwrap()
            .module(),
        decoded.root()
    );
    if bounded {
        let nested = owner
            .abi
            .private_types
            .iter()
            .find(|record| record.definition.name == "Nested")
            .unwrap();
        assert_eq!(nested.definition.kind, TypeDefKind::Enum);
        for record in [hidden, nested] {
            let parameters = &record.definition.generic_params;
            assert_eq!(parameters.len(), 1);
            assert_eq!(parameters[0].owner, record.declaration);
            assert_eq!(parameters[0].position, 0);
            let bounds = &record.definition.bounds;
            assert_eq!(bounds.len(), 1);
            assert_eq!(bounds[0].ty, parameters[0].as_type());
            assert_eq!(bounds[0].constraints.len(), 1);
        }
    }
    let bytecode = lower_program_to_bytecode(&decoded, &Default::default()).unwrap();
    let bytecode = bytecode.program();
    assert_eq!(
        bytecode.modules[bytecode.root.index()].private_types,
        owner.abi.private_types
    );
}

#[test]
fn private_nested_argument_bounds_are_checked_before_export_lowering() {
    let mut sources = SourceDatabase::default();
    let root = insert(
        &mut sources,
        "root",
        &BOUNDED_PRIVATE_APPLICATION.replace("value: 1", "value: true"),
    );
    let error = AnalysisDatabase::default()
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap()
        .check_program(root, &Default::default())
        .unwrap_err();
    let ProgramCheckError::Diagnostics(diagnostics) = error else {
        panic!("expected checked private argument bound failure: {error:?}")
    };
    assert!(
        diagnostics.iter().any(|diagnostic| matches!(
            &diagnostic.diagnostic.kind,
            DiagnosticKind::GenericBoundNotSatisfied { type_name, trait_name }
                if type_name == "bool" && trait_name.ends_with("Tag")
        )),
        "{diagnostics:?}"
    );
}

#[test]
fn unused_nonobject_exports_retain_enum_layouts_for_structural_bound_proofs() {
    let mut sources = SourceDatabase::default();
    let root = insert(
        &mut sources,
        "root",
        include_str!("../../../tests/fixtures/type_exports/enum-bound-alias.kgr"),
    );
    let checked = checked(&sources, root);
    let program = lower_program_to_mir(&checked, &Default::default()).unwrap();
    let bytes = encode_program(&program, &Default::default()).unwrap();
    let decoded = decode_program(&bytes, &Default::default()).unwrap();
    assert_eq!(
        bytes,
        encode_program(&decoded, &Default::default()).unwrap()
    );
    let TypeExport::Closed(Ty::Enum(wrapper)) =
        &decoded.exports().namespaces[decoded.root()].types["Exported"]
    else {
        panic!("export remains a non-object enum application")
    };
    let Ty::Enum(option) = &wrapper.arguments[0] else {
        panic!("structural Eq proof requires the concrete Option payload layout")
    };
    assert_eq!(option.arguments, [Ty::Builtin(BuiltinType::I32)]);
    for application in [wrapper, option] {
        let identity = decoded
            .definitions()
            .resolve(application.declaration)
            .unwrap();
        let owner = decoded
            .modules()
            .iter()
            .find(|module| &module.identity == identity.module())
            .unwrap();
        assert!(owner.enumerations.iter().any(|layout| {
            layout.declaration == application.declaration
                && layout.arguments == application.arguments
        }));
    }
    let root = decoded
        .modules()
        .iter()
        .find(|module| &module.identity == decoded.root())
        .unwrap();
    assert!(
        root.functions.is_empty(),
        "type proofs do not demand executable bodies"
    );
    lower_program_to_bytecode(&decoded, &Default::default()).unwrap();
}

#[test]
fn unused_closed_exports_demand_owner_layouts_and_transitive_private_fields() {
    let mut sources = SourceDatabase::default();
    insert(
        &mut sources,
        "types",
        include_str!("../../../tests/fixtures/type_exports/nested-owner.kgr"),
    );
    insert(
        &mut sources,
        "facade",
        include_str!("../../../tests/fixtures/type_exports/nested-facade.kgr"),
    );
    let root = insert(
        &mut sources,
        "root",
        include_str!("../../../tests/fixtures/type_exports/nested-root.kgr"),
    );
    let checked = AnalysisDatabase::default()
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap()
        .check_program(root, &Default::default())
        .unwrap();
    let options = MirLoweringOptions {
        max_generic_instances: 6,
        ..Default::default()
    };
    let program = lower_program_to_mir(&checked, &options).unwrap();
    let owner = program
        .modules()
        .iter()
        .find(|module| module.identity.path == ["types"])
        .unwrap();
    let root = program
        .modules()
        .iter()
        .find(|module| module.identity.path == ["root"])
        .unwrap();
    let exports = &program.exports().namespaces[&root.identity];
    assert_eq!(exports.types["Int"], exports.types["Again"]);
    assert_eq!(exports.types["Erased"], TypeExport::OpenAlias { arity: 1 });
    assert!(exports.modules.is_empty());
    assert!(!program.exports().namespaces.contains_key(&owner.identity));
    assert_eq!(
        owner
            .structures
            .iter()
            .filter(|layout| !layout.arguments.is_empty())
            .count(),
        6,
        "two object aliases, one nested struct application and their private field layouts"
    );
    let TypeExport::Closed(Ty::Tuple(wrapped)) = &exports.types["Wrapped"] else {
        panic!("non-object alias retains its checked type")
    };
    let TypeExport::Closed(int) = &exports.types["Int"] else {
        panic!("closed alias")
    };
    let TypeExport::Closed(boolean) = &exports.types["Bool"] else {
        panic!("closed alias")
    };
    for ty in [int, boolean, &wrapped[0]] {
        let Ty::Struct(ty) = ty else {
            panic!("closed script struct export")
        };
        let layout = owner
            .structures
            .iter()
            .find(|layout| layout.declaration == ty.declaration && layout.arguments == ty.arguments)
            .unwrap();
        let Ty::Struct(inner) = &layout.fields[0].ty else {
            panic!("transitive private field")
        };
        let inner = owner
            .structures
            .iter()
            .find(|layout| {
                layout.declaration == inner.declaration && layout.arguments == inner.arguments
            })
            .unwrap();
        assert_eq!(inner.fields[0].ty, ty.arguments[0]);
        assert_eq!(layout.arguments.len(), 1);
    }
    assert!(
        owner
            .structures
            .iter()
            .all(|layout| layout.arguments != [Ty::Builtin(BuiltinType::U32)])
    );
    assert!(
        program
            .modules()
            .iter()
            .all(|module| module.functions.is_empty())
    );
    assert!(
        program
            .modules()
            .iter()
            .filter(|module| module.identity.path != ["types"])
            .all(|module| module
                .structures
                .iter()
                .all(|layout| layout.arguments.is_empty()))
    );

    let error = lower_program_to_mir(
        &checked,
        &MirLoweringOptions {
            max_generic_instances: 5,
            ..options
        },
    )
    .unwrap_err();
    assert!(matches!(error, SourceProgramError::Lowering {
        error: MirLoweringError::Diagnostic(diagnostic), ..
    } if matches!(diagnostic.kind, DiagnosticKind::CompileLimitExceeded {
        resource: "generic instances", limit: 5
    })));
}

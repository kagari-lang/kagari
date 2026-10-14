//! Public-path evidence survives source-free MIR decoding and identity adoption.

use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionKind, ModuleIdentity, map::DefinitionContext, table::DefinitionId},
};
use kagari_compiler::source::program::lower_program_to_mir;
use kagari_contract::{exports::ExportNamespace, types::PublicItem};
use kagari_hir::analysis::AnalysisDatabase;
use kagari_mir::{
    codec::{decode_program, encode_program},
    program::{ProgramErrorKind, VerifiedMirProgram, records::MirProgram, verify_program},
};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_stdlib::catalog;
use kagari_types::{scalar::BuiltinType, ty::Ty};

fn compiled() -> VerifiedMirProgram {
    let mut source = SourceDatabase::default();
    let root = source
        .set(
            "exports.kgr",
            r#"
        mod hidden {
            pub fn calculate(value: i32) -> i32 { value + 1 }
            fn secret() -> i32 { 2 }
            pub fn main() -> i32 { 3 }
        }
        pub mod api { pub fn tick() -> i32 { 4 } }
        pub use crate::hidden::calculate as damage;
        pub use crate as again;
        pub fn api() -> bool { true }
        pub fn generic<T>(value: T) -> T { value }
        pub fn wrapper() -> i32 { generic(5) }
        fn main() -> i32 { 6 }
    "#
            .into(),
            SourceLayer::Base,
        )
        .unwrap();
    let mut analysis = AnalysisDatabase::default();
    analysis.set_native_modules(catalog::shared().unwrap());
    let checked = analysis
        .snapshot(source.snapshot(), &Default::default())
        .unwrap()
        .check_program(root, &Default::default())
        .unwrap();
    lower_program_to_mir(&checked, &Default::default()).unwrap()
}

fn function(program: &MirProgram, module: &[&str], name: &str) -> DefinitionId {
    program
        .modules
        .iter()
        .find(|owner| owner.identity.path == module)
        .unwrap()
        .functions
        .iter()
        .find(|function| function.name == name)
        .unwrap()
        .instance
        .declaration
}

#[test]
fn source_free_exports_preserve_aliases_cycles_and_separate_entry() {
    let program = compiled();
    let bytes = encode_program(&program, &Default::default()).unwrap();
    let decoded = decode_program(&bytes, &Default::default()).unwrap();
    assert_eq!(
        bytes,
        encode_program(&decoded, &Default::default()).unwrap()
    );
    let context = DefinitionContext::new().unwrap();
    let editable = decoded
        .into_unverified()
        .import_into(&context, &Default::default())
        .unwrap()
        .into_records();
    let root = &editable.root;
    let damage = function(&editable, &["hidden"], "calculate");
    assert_eq!(editable.exports.function(root, "damage"), Some(damage));
    assert_eq!(
        editable.exports.function(root, "again::again::damage"),
        Some(damage)
    );
    assert_eq!(
        editable.exports.function(root, "api"),
        Some(function(&editable, &[], "api"))
    );
    assert_eq!(
        editable.exports.function(root, "api::tick"),
        Some(function(&editable, &["api"], "tick"))
    );
    assert_eq!(
        editable.exports.function(root, "wrapper"),
        Some(function(&editable, &[], "wrapper"))
    );
    assert_eq!(
        editable.exports.entry,
        Some(function(&editable, &[], "main"))
    );
    for private in [
        "main",
        "hidden::calculate",
        "secret",
        "generic",
        "::damage",
        "damage::",
    ] {
        assert_eq!(editable.exports.function(root, private), None, "{private}");
    }
    assert_eq!(
        editable
            .exports
            .function(root, &format!("{}damage", "again::".repeat(64))),
        None
    );
    verify_program(editable, &context, &Default::default()).unwrap();
}

#[test]
fn executable_export_validation_rejects_forged_authority_and_dangling_graphs() {
    let program = compiled();
    let bytes = encode_program(&program, &Default::default()).unwrap();
    let context = DefinitionContext::new().unwrap();
    let raw = decode_program(&bytes, &Default::default())
        .unwrap()
        .into_unverified()
        .import_into(&context, &Default::default())
        .unwrap()
        .into_records();
    for corruption in 0..7 {
        let mut forged = raw.clone();
        let hidden = function(&forged, &["hidden"], "secret");
        let foreign_main = function(&forged, &["hidden"], "main");
        let wrapper = function(&forged, &[], "wrapper");
        match corruption {
            0 => {
                forged
                    .exports
                    .namespaces
                    .get_mut(&forged.root)
                    .unwrap()
                    .functions
                    .insert("stolen".into(), hidden);
            }
            1 => {
                forged.exports.entry = Some(foreign_main);
            }
            2 => {
                forged.exports.entry = Some(wrapper);
            }
            3 => {
                forged
                    .exports
                    .namespaces
                    .get_mut(&forged.root)
                    .unwrap()
                    .modules
                    .insert("missing".into(), ModuleIdentity::single_file("missing"));
            }
            4 => {
                forged.exports.namespaces.insert(
                    ModuleIdentity::single_file("unreachable"),
                    ExportNamespace::default(),
                );
            }
            5 => {
                let absent = context
                    .intern_child(
                        context.intern_root(&forged.root).unwrap(),
                        DefinitionKind::Function,
                        "absent",
                        0,
                    )
                    .unwrap();
                forged
                    .exports
                    .namespaces
                    .get_mut(&forged.root)
                    .unwrap()
                    .functions
                    .insert("absent".into(), absent);
            }
            6 => {
                let module = forged
                    .modules
                    .iter_mut()
                    .find(|module| module.identity.path == ["hidden"])
                    .unwrap();
                let item = module
                    .abi
                    .public_items
                    .iter_mut()
                    .find_map(|item| match item {
                        PublicItem::Function(function) if function.name == "calculate" => {
                            Some(function)
                        }
                        _ => None,
                    })
                    .unwrap();
                item.return_type = Ty::Builtin(BuiltinType::Bool);
            }
            _ => unreachable!(),
        }
        assert!(
            matches!(
                verify_program(forged, &context, &Default::default())
                    .unwrap_err()
                    .kind,
                ProgramErrorKind::InvalidExports
            ),
            "corruption {corruption}"
        );
    }
    let cancel = CancellationToken::default();
    cancel.cancel();
    assert!(matches!(
        verify_program(raw, &context, &cancel).unwrap_err().kind,
        ProgramErrorKind::Cancelled
    ));
}

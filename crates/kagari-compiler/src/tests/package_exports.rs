//! Public-path evidence survives source-free MIR/KBC decoding and identity adoption.

use crate::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_bytecode::{
    artifact::KbcArtifact,
    program::{identity, verify_program as verify_bytecode},
    verifier::BytecodeVerificationError,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionKind, ModuleIdentity, map::DefinitionContext, table::DefinitionId},
};
use kagari_contract::{exports::ExportNamespace, types::PublicItem};
use kagari_hir::analysis::AnalysisDatabase;
use kagari_mir::{
    codec::{decode_program, encode_program},
    program::{ProgramErrorKind, VerifiedMirProgram, records::MirProgram, verify_program},
};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_types::{scalar::BuiltinType, ty::Ty};

fn compiled() -> VerifiedMirProgram {
    let mut source = SourceDatabase::default();
    let root = source
        .set(
            "exports.kgr",
            r#"
        mod hidden {
            pub fn calculate(value: i32) -> i32 { value }
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
    let mir = verify_program(editable, &context, &Default::default()).unwrap();
    let bytecode = lower_program_to_bytecode(&mir, &Default::default())
        .map(|program| program.into_unverified())
        .unwrap()
        .into_records();
    let artifact =
        KbcArtifact::from_program(bytecode, Default::default(), &context, &Default::default())
            .unwrap();
    let bytes = artifact.to_bytes(&context.snapshot()).unwrap();
    let fresh = DefinitionContext::new().unwrap();
    let artifact = KbcArtifact::from_bytes(&bytes)
        .unwrap()
        .import_into(&fresh, &Default::default())
        .unwrap()
        .into_records();
    assert_eq!(bytes, artifact.to_bytes(&fresh.snapshot()).unwrap());
    let verified = artifact
        .into_verified(&Default::default(), &fresh, &Default::default())
        .unwrap()
        .into_bytecode();
    let program = verified.program();
    let root = &program.modules[program.root.index()].identity;
    for (path, name) in [
        ("damage", "calculate"),
        ("again::damage", "calculate"),
        ("api", "api"),
        ("api::tick", "tick"),
        ("wrapper", "wrapper"),
    ] {
        let id = program.exports.function(root, path).unwrap();
        let target = verified.export_targets()[&id];
        let body = &program.modules[target.module.index()].functions[target.function.index()];
        assert_eq!(body.name, name);
        assert_eq!(body.identity.as_ref().unwrap().declaration, id);
        assert!(body.identity.as_ref().unwrap().arguments.is_empty());
    }
    assert!(program.exports.function(root, "main").is_none());
    let entry = verified.export_targets()[&program.exports.entry.unwrap()];
    assert_eq!(entry.module, program.root);
    assert_eq!(
        program.modules[entry.module.index()].functions[entry.function.index()].name,
        "main"
    );
}

#[test]
fn executable_export_validation_rejects_forged_authority_and_dangling_graphs() {
    let program = compiled();
    let bytes = encode_program(&program, &Default::default()).unwrap();
    let context = DefinitionContext::new().unwrap();
    let bytecode = lower_program_to_bytecode(&program, &Default::default())
        .map(|program| program.into_unverified())
        .unwrap()
        .import_into(&context, &Default::default())
        .unwrap()
        .into_records();
    identity::collect(
        &bytecode.modules.iter().collect::<Vec<_>>(),
        &context,
        &Default::default(),
    )
    .unwrap();
    verify_bytecode(&bytecode, &context.snapshot(), &Default::default()).unwrap();
    let raw = decode_program(&bytes, &Default::default())
        .unwrap()
        .into_unverified()
        .import_into(&context, &Default::default())
        .unwrap()
        .into_records();
    for corruption in 0..8 {
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
            7 => {
                forged
                    .modules
                    .iter_mut()
                    .find(|module| module.identity == forged.root)
                    .unwrap()
                    .functions
                    .iter_mut()
                    .find(|function| function.name == "main")
                    .unwrap()
                    .semantic
                    .result = None;
            }
            _ => unreachable!(),
        }
        let mut forged_bytecode = bytecode.clone();
        forged_bytecode.exports = forged.exports.clone();
        if corruption == 6 {
            let module = forged_bytecode
                .modules
                .iter_mut()
                .find(|module| module.identity.path == ["hidden"])
                .unwrap();
            let item = module
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
        if corruption == 7 {
            forged_bytecode.modules[forged_bytecode.root.index()]
                .functions
                .iter_mut()
                .find(|function| function.name == "main")
                .unwrap()
                .metadata
                .semantic
                .result = None;
        }
        let error = verify_bytecode(&forged_bytecode, &context.snapshot(), &Default::default())
            .unwrap_err();
        if matches!(corruption, 6 | 7) {
            // Bytecode's existing semantic-flow verifier detects the signature
            // mismatch or omission before the final export binding check.
            assert!(matches!(
                error,
                BytecodeVerificationError::InvalidOperation {
                    reason: "invalid collection access flow",
                    ..
                }
            ));
        } else {
            assert_eq!(
                error,
                BytecodeVerificationError::InvalidProgramExports,
                "bytecode corruption {corruption}"
            );
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

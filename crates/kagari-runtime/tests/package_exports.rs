//! Public paths retain verified targets across context import and generation changes.

use kagari_bytecode::instruction::BytecodeInstruction;
use kagari_common::identity::map::DefinitionContext;
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_hir::analysis::AnalysisDatabase;
use kagari_runtime::{
    Runtime,
    module::{ModuleEpochRetention, VerifiedProgram},
    value::Value,
};
use kagari_source::source_database::{SourceDatabase, SourceLayer};

fn code(value: i32) -> VerifiedProgram {
    let mut source = SourceDatabase::default();
    let root = source
        .set(
            "exports.kgr",
            format!(
                r#"
        mod hidden {{ pub fn calculate() -> i32 {{ {value} }} fn secret() {{ }} }}
        pub use crate::hidden::calculate as damage;
        pub use crate as again;
        fn main() {{ }}
    "#
            ),
            SourceLayer::Base,
        )
        .unwrap();
    let mut analysis = AnalysisDatabase::default();
    let checked = analysis
        .snapshot(source.snapshot(), &Default::default())
        .unwrap()
        .check_program(root, &Default::default())
        .unwrap();
    let mir = lower_program_to_mir(&checked, &Default::default()).unwrap();
    let bytecode = lower_program_to_bytecode(&mir).unwrap();
    VerifiedProgram::new(bytecode, &DefinitionContext::new().unwrap()).unwrap()
}

#[test]
fn public_paths_remain_rooted_and_generation_pinned_after_identity_import() {
    let original = code(7);
    let target = original.exported_function("damage").unwrap();
    assert_eq!(original.exported_function("again::damage"), Some(target));
    assert!(original.exported_function("main").is_none());
    assert!(original.entry_function().is_some());
    let mut first = Runtime::default();
    let mut second = Runtime::default();
    let loaded = first
        .load_verified_program("exports", original.clone())
        .unwrap();
    let foreign = second
        .load_verified_program("exports", original.clone())
        .unwrap();
    assert_ne!(loaded.definitions().id(), foreign.definitions().id());
    assert!(
        loaded
            .verified_program()
            .same_version(foreign.verified_program())
    );
    for root in [&loaded, &foreign] {
        let (owner, function) = root.exported_function("damage").unwrap();
        assert_eq!(owner.bytecode.identity.path, ["hidden"]);
        assert_eq!(owner.bytecode.functions[function.index()].name, "calculate");
        let declaration = owner.bytecode.functions[function.index()]
            .identity
            .as_ref()
            .unwrap()
            .declaration;
        assert_eq!(
            owner.definitions().resolve(declaration).unwrap().module(),
            &owner.bytecode.identity
        );
        assert!(root.exported_function("hidden::calculate").is_none());
        assert!(owner.exported_function("secret").is_none());
        assert!(owner.exported_function("calculate").is_none());
        assert_eq!(
            owner.exported_function("again::damage").unwrap().0.key(),
            owner.key()
        );
        let (entry, function) = owner.entry_function().unwrap();
        assert_eq!(entry.key(), root.key());
        assert_eq!(entry.bytecode.functions[function.index()].name, "main");
    }
    let (old_target, old_function) = loaded.exported_function("damage").unwrap();
    let _lease = first
        .retain_module(&old_target, ModuleEpochRetention::ActiveCall)
        .unwrap();
    let staged = first
        .stage_reload_verified_program(&loaded, "exports", code(9))
        .unwrap();
    let published = first.publish_staged_reload(staged).unwrap();
    let (new_target, new_function) = published.exported_function("damage").unwrap();
    assert_ne!(old_target.key(), new_target.key());
    assert_ne!(
        loaded.program_fingerprint(),
        published.program_fingerprint()
    );
    assert_eq!(
        loaded.exported_function("damage").unwrap().0.key(),
        old_target.key()
    );
    for (owner, function, expected) in [
        (&old_target, old_function, 7),
        (&new_target, new_function, 9),
    ] {
        let BytecodeInstruction::LoadConst { constant, .. } =
            owner.bytecode.functions[function.index()].instructions[0]
        else {
            panic!("constant-return fixture");
        };
        assert_eq!(
            first.read_constant(owner, constant).unwrap(),
            Value::I32(expected)
        );
    }
    first.validate_loaded_module(&old_target).unwrap();
    first.validate_loaded_module(&new_target).unwrap();
    assert!(second.validate_loaded_module(&old_target).is_err());
}

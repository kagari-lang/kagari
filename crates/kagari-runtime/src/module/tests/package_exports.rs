//! Public paths retain verified targets across context import and generation changes.

use crate::{
    Runtime,
    module::{ModuleEpochRetention, VerifiedProgram},
    reload::{
        ReloadValidationError, validate_reload_artifact_candidate, validate_reload_candidate,
    },
    value::Value,
};
use kagari_bytecode::{
    artifact::KbcArtifact, instruction::BytecodeInstruction, program::BytecodeProgram,
};
use kagari_common::identity::{map::DefinitionContext, metadata::DefinitionMetadata};
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_hir::analysis::AnalysisDatabase;
use kagari_source::source_database::{SourceDatabase, SourceLayer};

fn code(value: i32) -> VerifiedProgram {
    source_code(&format!(
        r#"
        mod hidden {{ pub fn calculate() -> i32 {{ {value} }} fn secret() {{ }} }}
        pub use crate::hidden::calculate as damage;
        pub use crate as again;
        fn main() {{ }}
    "#
    ))
}

fn source_code(text: &str) -> VerifiedProgram {
    VerifiedProgram::new(bytecode(text), &DefinitionContext::new().unwrap()).unwrap()
}

fn bytecode(text: &str) -> DefinitionMetadata<BytecodeProgram> {
    let mut source = SourceDatabase::default();
    let root = source
        .set("exports.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut analysis = AnalysisDatabase::default();
    let checked = analysis
        .snapshot(source.snapshot(), &Default::default())
        .unwrap()
        .check_program(root, &Default::default())
        .unwrap();
    let mir = lower_program_to_mir(&checked, &Default::default()).unwrap();
    lower_program_to_bytecode(&mir, &Default::default())
        .map(|program| program.into_unverified())
        .unwrap()
}

#[test]
fn member_counts_distinguish_local_and_whole_program_leases() {
    let mut runtime = Runtime::default();
    let root = runtime.load_verified_program("exports", code(7)).unwrap();
    let member = root
        .members()
        .find(|member| member.key() != root.key())
        .unwrap();
    let whole = runtime
        .retain_program(&member, ModuleEpochRetention::CompiledArtifact)
        .unwrap();
    let clone = whole.clone();
    let local = runtime
        .retain_module(&member, ModuleEpochRetention::RuntimeValue)
        .unwrap();
    for loaded in root.members() {
        let counts = runtime.modules().retention_counts(loaded.key());
        assert_eq!(counts.compiled_artifacts, 1);
        assert_eq!(
            counts.runtime_values,
            usize::from(loaded.key() == member.key())
        );
    }
    drop(whole);
    assert_eq!(
        runtime
            .modules()
            .retention_counts(root.key())
            .compiled_artifacts,
        1
    );
    drop(clone);
    assert_eq!(runtime.modules().retention_counts(root.key()).total(), 0);
    assert_eq!(
        runtime
            .modules()
            .retention_counts(member.key())
            .runtime_values,
        1
    );
    drop(local);
    assert!(
        root.members()
            .all(|member| runtime.modules().retention_counts(member.key()).total() == 0)
    );
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

fn reload_source(next: &str, target: &str, alias: &str, nominal: &str, entry: &str) -> String {
    format!(
        r#"
        pub mod one {{ pub fn run() -> i32 {{ 1 }} pub use crate::{next} as next; }}
        pub mod two {{ pub fn run() -> i32 {{ 2 }} pub use crate::one as next; }}
        mod implementation {{
            pub struct A {{ }}
            pub struct B {{ }}
            pub fn a() -> A {{ A {{ }} }}
            pub fn b() -> B {{ B {{ }} }}
            pub fn integer() -> i32 {{ 3 }}
            pub fn replacement() -> i32 {{ 4 }}
            pub fn boolean() -> bool {{ true }}
        }}
        pub use crate::one as api;
        pub use crate::implementation::{target} as {alias};
        pub use crate::implementation::{nominal} as make;
        {entry}
    "#
    )
}

#[test]
fn reload_compares_public_path_contracts_without_expanding_alias_cycles() {
    let original = source_code(&reload_source(
        "one",
        "integer",
        "value",
        "a",
        "fn main() {}",
    ));
    let mut runtime = Runtime::default();
    let loaded = runtime.load_verified_program("exports", original).unwrap();
    for source in [
        reload_source("one", "integer", "renamed", "a", "fn main() {}"),
        reload_source("one", "boolean", "value", "a", "fn main() {}"),
        reload_source("one", "integer", "value", "b", "fn main() {}"),
        reload_source("one", "integer", "value", "a", ""),
        reload_source("one", "integer", "value", "a", "fn main() -> i32 { 0 }"),
        reload_source("one", "integer", "value", "a", "fn main() {}")
            .replace("as api", "as renamed"),
    ] {
        assert!(matches!(
            runtime.stage_reload_verified_program(&loaded, "exports", source_code(&source)),
            Err(ReloadValidationError::PublicExportMismatch)
        ));
    }
    // One-state and two-state namespace cycles expose the same path language.
    // Retargeting a public leaf to a same-signature declaration is compatible.
    let candidate = source_code(&reload_source(
        "two",
        "replacement",
        "value",
        "a",
        "fn main() {}",
    ));
    let old = loaded.exported_function("api::next::run").unwrap().0;
    let _lease = runtime
        .retain_module(&old, ModuleEpochRetention::ActiveCall)
        .unwrap();
    let staged = runtime
        .stage_reload_verified_program(&loaded, "exports", candidate)
        .unwrap();
    let published = runtime.publish_staged_reload(staged).unwrap();
    assert_eq!(old.bytecode.identity.path, ["one"]);
    assert_eq!(
        published
            .exported_function("api::next::run")
            .unwrap()
            .0
            .bytecode
            .identity
            .path,
        ["two"]
    );
    assert_eq!(
        loaded.exported_function("api::next::run").unwrap().0.key(),
        old.key()
    );
    let (member, function) = published.exported_function("value").unwrap();
    assert_eq!(
        member.bytecode.functions[function.index()].name,
        "replacement"
    );
}

#[test]
fn source_free_reload_validators_use_the_same_public_contract() {
    let original = source_code(&reload_source(
        "one",
        "integer",
        "value",
        "a",
        "fn main() {}",
    ));
    let mut runtime = Runtime::default();
    let loaded = runtime.load_verified_program("exports", original).unwrap();
    for compatible in [true, false] {
        let alias = if compatible { "value" } else { "renamed" };
        let context = DefinitionContext::new().unwrap();
        let program = bytecode(&reload_source(
            "two",
            "replacement",
            alias,
            "a",
            "fn main() {}",
        ))
        .import_into(&context, &Default::default())
        .unwrap()
        .into_records();
        let artifact = KbcArtifact::from_program(
            program.clone(),
            Default::default(),
            &context,
            &Default::default(),
        )
        .unwrap();
        let raw = validate_reload_candidate(&loaded, "exports", program, Some(&loaded), &context);
        let bytes = artifact.to_bytes(&context.snapshot()).unwrap();
        let decoded_context = DefinitionContext::new().unwrap();
        let decoded = KbcArtifact::from_bytes(&bytes)
            .unwrap()
            .import_into(&decoded_context, &Default::default())
            .unwrap()
            .into_records();
        let artifact = validate_reload_artifact_candidate(
            &loaded,
            "exports",
            decoded,
            &Default::default(),
            Some(&loaded),
            &decoded_context,
        );
        if compatible {
            raw.unwrap();
            artifact.unwrap();
        } else {
            assert!(matches!(
                raw,
                Err(ReloadValidationError::PublicExportMismatch)
            ));
            assert!(matches!(
                artifact,
                Err(ReloadValidationError::PublicExportMismatch)
            ));
        }
    }
}

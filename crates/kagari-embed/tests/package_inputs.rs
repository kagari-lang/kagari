#![cfg(feature = "source")]

use kagari_common::cancellation::CancellationToken;
use kagari_embed::{
    BytecodeArtifact,
    engine::{KagariEngine, source::CheckedModule},
    error::EmbeddingError,
    program::PreparedProgram,
};
use kagari_package::{
    filesystem::FilesystemSources,
    input::InputTree,
    lock::ResolutionOptions,
    modules::VirtualSources,
    project::{ProjectInputs, ProjectOptions},
    snapshot::ProgramInputs,
};
use kagari_runtime::module::entry::ProgramEntry;
use std::{
    fs,
    path::PathBuf,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

fn files(base: i32) -> Vec<(&'static str, String)> {
    vec![
        (
            "Kagari.toml",
            "[workspace]\nnamespace = 'tests/package-sdk'\nmembers = ['app', 'other']".into(),
        ),
        (
            "app/Kagari.toml",
            "[package]\nname = 'app'\nversion = '1.0.0'\n[dependencies]\nrules = { path = '../rules' }".into(),
        ),
        (
            "app/src/lib.kgr",
            "pub fn answer() -> i32 { rules::value(()) }\npub use rules::plain as dependency;\nfn main() -> i32 { answer() }".into(),
        ),
        (
            "rules/Kagari.toml",
            "[package]\nname = 'rules'\nversion = '1.0.0'".into(),
        ),
        (
            "rules/src/lib.kgr",
            format!("const BASE: i32 = {base};\npub fn value<T>(ignored: T) -> i32 {{ BASE }}\npub fn plain() -> i32 {{ 9 }}"),
        ),
        (
            "other/Kagari.toml",
            "[package]\nname = 'other'\nversion = '1.0.0'".into(),
        ),
        (
            "other/src/lib.kgr",
            "fn broken() -> i32 { true }".into(),
        ),
    ]
}

fn capture(input: &impl InputTree, member: &str) -> ProgramInputs {
    ProjectInputs::load(input, member, &Default::default(), &Default::default())
        .unwrap()
        .programs()[member]
        .clone()
}

fn virtual_inputs(base: i32) -> VirtualSources {
    VirtualSources::new(
        files(base)
            .into_iter()
            .map(|(name, text)| (name.into(), Arc::from(text))),
        &Default::default(),
    )
    .unwrap()
}

fn prepare(engine: &KagariEngine, checked: &CheckedModule) -> PreparedProgram {
    let artifact = engine.emit_bytecode(checked, Default::default()).unwrap();
    let bytes = artifact.records().to_bytes(artifact.definitions()).unwrap();
    PreparedProgram::from_artifact(
        BytecodeArtifact::from_bytes(&bytes).unwrap(),
        &Default::default(),
        &Default::default(),
    )
    .unwrap()
}

#[test]
fn captured_packages_share_the_sdk_pipeline_and_retain_generations() {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/package-sdk")
        .join(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
                .to_string(),
        );
    for (name, text) in files(42) {
        let path = directory.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    let disk = FilesystemSources::new(&directory).unwrap();
    let project =
        ProjectInputs::load(&disk, "app", &Default::default(), &Default::default()).unwrap();
    assert_eq!(project.programs().len(), 1);
    assert_eq!(project.graph().members().len(), 3);
    project
        .lockfile()
        .publish(
            &disk,
            &project.workspace().directory,
            Default::default(),
            &Default::default(),
        )
        .unwrap();
    let locked = ProjectOptions {
        resolution: ResolutionOptions {
            locked: true,
            offline: true,
        },
        ..Default::default()
    };
    let project = ProjectInputs::load(&disk, "app", &locked, &Default::default()).unwrap();
    let inputs = &project.programs()["app"];
    let memory = capture(&virtual_inputs(42), "app");
    assert_eq!(inputs.root(), memory.root());
    assert_eq!(inputs.fingerprint(), memory.fingerprint());
    let engine = KagariEngine::default();
    let cancel = CancellationToken::default();
    let original = engine.compile_package(inputs, &cancel).unwrap();
    assert!(original.module_identity().path.is_empty());
    assert_eq!(
        original.package_inputs().unwrap().fingerprint(),
        inputs.fingerprint()
    );
    let snapshot = engine.source_snapshot();
    let root = snapshot.module(original.module_identity()).unwrap().clone();
    let unchanged = engine.compile_package(inputs, &cancel).unwrap();
    assert!(Arc::ptr_eq(
        engine
            .source_snapshot()
            .module(unchanged.module_identity())
            .unwrap(),
        &root
    ));

    let cancelled = CancellationToken::default();
    cancelled.cancel();
    assert!(matches!(
        engine.compile_package(&memory, &cancelled),
        Err(EmbeddingError::Cancelled)
    ));
    assert_eq!(engine.source_snapshot().revision(), snapshot.revision());

    let changed_inputs = capture(&virtual_inputs(43), "app");
    assert_ne!(inputs.fingerprint(), changed_inputs.fingerprint());
    let changed = engine.compile_package(&changed_inputs, &cancel).unwrap();
    // Workspace membership does not check or expose another root's source bodies.
    let invalid = capture(&virtual_inputs(43), "other");
    assert!(matches!(
        engine.compile_package(&invalid, &cancel),
        Err(EmbeddingError::Diagnostics { .. })
    ));
    assert!(snapshot.module(original.module_identity()).is_some());

    // Both retained results emit after the engine has moved to an unrelated root.
    // The runtime consumes decoded artifacts and never receives package inputs.
    let old_program = prepare(&engine, &original);
    let new_program = prepare(&engine, &changed);
    let mut runtime = engine.runtime(Default::default());
    let loaded = runtime
        .load_program(&old_program, Default::default())
        .unwrap();
    let pinned = runtime
        .runtime()
        .bind_function::<(), i32>(&loaded, "answer")
        .unwrap();
    assert_eq!(runtime.call(&pinned, (), &Default::default()).unwrap(), 42);
    assert_eq!(
        runtime
            .execute_typed::<(), i32>(&loaded, "dependency", (), &Default::default())
            .unwrap(),
        9
    );
    assert!(
        runtime
            .execute_typed::<(), i32>(&loaded, "rules::plain", (), &Default::default())
            .is_err()
    );
    let reloaded = runtime
        .reload_program(&loaded, &new_program, Default::default())
        .unwrap();
    runtime.runtime().collect_garbage().unwrap();
    assert_eq!(
        runtime
            .execute_typed::<(), i32>(&reloaded, ProgramEntry::Main, (), &Default::default())
            .unwrap(),
        43
    );
    assert_eq!(runtime.call(&pinned, (), &Default::default()).unwrap(), 42);
}

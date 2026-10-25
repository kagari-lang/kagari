#![cfg(feature = "source")]

use crate::{
    BytecodeArtifact,
    engine::{KagariEngine, packages::PackageBuildSet, source::CheckedModule},
    error::EmbeddingError,
    program::PreparedProgram,
};
use kagari_common::cancellation::CancellationToken;
use kagari_package::{
    filesystem::FilesystemSources,
    input::InputTree,
    lock::ResolutionOptions,
    modules::VirtualSources,
    project::{ProjectInputs, ProjectOptions},
    snapshot::ProgramInputs,
    workspace::MemberSelection,
};
use kagari_runtime::{
    module::entry::ProgramEntry,
    native::{
        builder::ModuleBuilder, context::CallContext, declarations::FunctionDecl, types::Type,
    },
};
use kagari_stdlib::declarations::StandardDeclarations;
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
            format!("const BASE: i32 = 42;\npub fn value<T>(ignored: T) -> i32 {{ BASE + {} }}\npub fn plain() -> i32 {{ 9 }}", base - 42),
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
    let artifact = engine.emit_artifact(checked, Default::default()).unwrap();
    let bytes = artifact.records().to_bytes(artifact.definitions()).unwrap();
    PreparedProgram::from_artifact(
        BytecodeArtifact::from_bytes(&bytes).unwrap(),
        &Default::default(),
        &Default::default(),
    )
    .unwrap()
}

#[test]
fn captured_packages_share_the_sdk_pipeline_and_select_current_calls() {
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
    let initial = PackageBuildSet::capture(&engine, &project, &cancel).unwrap();
    let original = engine.compile_package(inputs, &cancel).unwrap();
    let fingerprint = engine.package_fingerprint(inputs, &cancel).unwrap();
    assert_eq!(original.package_fingerprint(), Some(fingerprint.as_str()));
    assert_eq!(
        fingerprint,
        engine.package_fingerprint(&memory, &cancel).unwrap()
    );
    assert!(original.module_identity().path.is_empty());
    assert_eq!(
        original.package_inputs().unwrap().fingerprint(),
        inputs.fingerprint()
    );
    let snapshot = engine.source_snapshot();
    let root = snapshot.module(original.module_identity()).unwrap().clone();
    let unchanged = engine.compile_package(inputs, &cancel).unwrap();
    assert_eq!(
        original.package_fingerprint(),
        unchanged.package_fingerprint()
    );
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

    let changed_project =
        ProjectInputs::load(&virtual_inputs(43), "app", &Default::default(), &cancel).unwrap();
    let next = PackageBuildSet::capture(&engine, &changed_project, &cancel).unwrap();
    let changes = next.changes_since(&initial);
    assert!(changes.removed.is_empty());
    assert_eq!(changes.rebuild, [inputs.root().family.clone()].into());
    let candidate = &next.programs()[changes.rebuild.first().unwrap()];
    let changed_inputs = candidate.sources();
    assert_ne!(inputs.fingerprint(), changed_inputs.fingerprint());
    let changed = engine.compile_package(changed_inputs, &cancel).unwrap();
    assert_eq!(changed.package_fingerprint(), Some(candidate.fingerprint()));
    assert_ne!(
        original.package_fingerprint(),
        changed.package_fingerprint()
    );
    // Constants remain acquisition inputs even though compatible publication
    // freezes their values. Keep invalidation and publication as separate checks.
    let mut constant_files = files(43);
    let rules = &mut constant_files
        .iter_mut()
        .find(|(name, _)| *name == "rules/src/lib.kgr")
        .unwrap()
        .1;
    assert_eq!(rules.matches("const BASE: i32 = 42;").count(), 1);
    *rules = rules.replace("const BASE: i32 = 42;", "const BASE: i32 = 43;");
    let constant_input = VirtualSources::new(
        constant_files
            .into_iter()
            .map(|(name, text)| (name.into(), Arc::from(text))),
        &cancel,
    )
    .unwrap();
    let constant_project =
        ProjectInputs::load(&constant_input, "app", &Default::default(), &cancel).unwrap();
    let constant_set = PackageBuildSet::capture(&engine, &constant_project, &cancel).unwrap();
    assert_eq!(
        constant_set.changes_since(&next).rebuild,
        [inputs.root().family.clone()].into()
    );
    let constant_changed = engine
        .compile_package(
            constant_set.programs()[&inputs.root().family].sources(),
            &cancel,
        )
        .unwrap();
    // Workspace membership does not check or expose another root's source bodies.
    let invalid = capture(&virtual_inputs(43), "other");
    assert!(matches!(
        engine.compile_package(&invalid, &cancel),
        Err(EmbeddingError::Diagnostics { .. })
    ));
    assert!(snapshot.module(original.module_identity()).is_some());

    // Retained results emit after the engine has moved to an unrelated root.
    // The runtime consumes decoded artifacts and never receives package inputs.
    let old_program = prepare(&engine, &original);
    let new_program = prepare(&engine, &changed);
    let constant_program = prepare(&engine, &constant_changed);
    let mut runtime = engine.runtime(Default::default());
    let loaded = runtime
        .load_program(&old_program, Default::default())
        .unwrap();
    let retained = runtime
        .runtime()
        .bind_function::<(), i32>(&loaded.root(), "answer")
        .unwrap();
    assert_eq!(
        runtime
            .call_with_context(&retained, (), &Default::default())
            .unwrap(),
        42
    );
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
    let before = runtime.runtime().select_program(&loaded).unwrap().key();
    assert_eq!(
        runtime
            .reload_program(&loaded, &constant_program, Default::default())
            .unwrap_err()
            .code(),
        "KG_RELOAD_DECLARATION_CONTRACT_MISMATCH"
    );
    assert_eq!(
        runtime.runtime().select_program(&loaded).unwrap().key(),
        before
    );
    assert_eq!(
        runtime
            .call_with_context(&retained, (), &Default::default())
            .unwrap(),
        42
    );
    runtime
        .reload_program(&loaded, &new_program, Default::default())
        .unwrap();
    let reloaded = loaded.clone();
    runtime.runtime().collect_garbage().unwrap();
    assert_eq!(
        runtime
            .execute_typed::<(), i32>(&reloaded, ProgramEntry::Main, (), &Default::default())
            .unwrap(),
        43
    );
    assert_eq!(
        runtime
            .call_with_context(&retained, (), &Default::default())
            .unwrap(),
        43
    );
    // Changing only the generic implementation must rebuild its callers too;
    // signatures and the dependency constant are unchanged in this generation.
    let mut generic_files = files(43);
    let rules = &mut generic_files
        .iter_mut()
        .find(|(name, _)| *name == "rules/src/lib.kgr")
        .unwrap()
        .1;
    assert_eq!(rules.matches("{ BASE + 1 }").count(), 1);
    *rules = rules.replace("{ BASE + 1 }", "{ BASE + 2 }");
    let generic_input = VirtualSources::new(
        generic_files
            .into_iter()
            .map(|(name, text)| (name.into(), Arc::from(text))),
        &cancel,
    )
    .unwrap();
    let generic_project =
        ProjectInputs::load(&generic_input, "app", &Default::default(), &cancel).unwrap();
    let generic_set = PackageBuildSet::capture(&engine, &generic_project, &cancel).unwrap();
    let generic_changes = generic_set.changes_since(&next);
    assert_eq!(
        generic_changes.rebuild,
        [inputs.root().family.clone()].into()
    );
    let generic = engine
        .compile_package(
            generic_set.programs()[generic_changes.rebuild.first().unwrap()].sources(),
            &cancel,
        )
        .unwrap();
    let generic_program = prepare(&engine, &generic);
    runtime
        .reload_program(&reloaded, &generic_program, Default::default())
        .unwrap();
    let generic_loaded = reloaded.clone();
    assert_eq!(
        runtime
            .execute_typed::<(), i32>(&generic_loaded, "answer", (), &Default::default())
            .unwrap(),
        44
    );
    assert_eq!(
        runtime
            .call_with_context(&retained, (), &Default::default())
            .unwrap(),
        44
    );
    let mut builder = ModuleBuilder::new(
        "unused",
        &StandardDeclarations::default().catalog().unwrap(),
    )
    .unwrap();
    let number = builder
        .define_function(FunctionDecl::new("number").returns(Type::i32()))
        .unwrap();
    builder
        .bind(number, |_: &mut CallContext<'_>| Ok(1i32))
        .unwrap();
    let configured =
        KagariEngine::with_native_modules(Default::default(), vec![builder.finish().unwrap()])
            .unwrap();
    let configured_fingerprint = configured.package_fingerprint(inputs, &cancel).unwrap();
    assert_ne!(
        fingerprint, configured_fingerprint,
        "provider changes invalidate even unchanged source"
    );
    assert_eq!(
        original.package_fingerprint(),
        Some(fingerprint.as_str()),
        "checked input identity is retained"
    );
    let isolated = configured.new_session(Default::default());
    assert_eq!(
        configured_fingerprint,
        isolated.package_fingerprint(inputs, &cancel).unwrap(),
        "fresh sessions retain only native providers"
    );
}

#[test]
fn selected_root_changes_follow_transitive_inputs_and_provider_contracts() {
    let engine = KagariEngine::default();
    let capture = |files: Vec<(&str, String)>| {
        let input = VirtualSources::new(
            files
                .into_iter()
                .map(|(name, text)| (name.into(), Arc::from(text))),
            &Default::default(),
        )
        .unwrap();
        let project = ProjectInputs::load(
            &input,
            "",
            &ProjectOptions {
                selection: MemberSelection::Workspace,
                ..Default::default()
            },
            &Default::default(),
        )
        .unwrap();
        PackageBuildSet::capture(&engine, &project, &Default::default()).unwrap()
    };
    let initial = capture(files(42));
    let same = capture(files(42));
    assert!(same.changes_since(&initial).rebuild.is_empty());
    for (path, from, to, expected) in [
        (
            "rules/src/lib.kgr",
            "BASE + 0 }",
            "BASE + 1 }",
            vec!["app", "rules"],
        ),
        ("rules/src/lib.kgr", "{ 9 }", "{ 10 }", vec!["app", "rules"]),
        (
            "rules/src/lib.kgr",
            "BASE: i32 = 42",
            "BASE: i32 = 43",
            vec!["app", "rules"],
        ),
        ("rules/Kagari.toml", "1.0.0", "1.1.0", vec!["app", "rules"]),
        (
            "app/Kagari.toml",
            "rules = {",
            "renamed = { package = 'rules',",
            vec!["app"],
        ),
    ] {
        let mut input = files(42);
        let text = &mut input.iter_mut().find(|(name, _)| *name == path).unwrap().1;
        assert!(text.contains(from));
        *text = text.replace(from, to);
        let changes = capture(input).changes_since(&initial);
        let mut names = changes
            .rebuild
            .iter()
            .map(|family| family.name())
            .collect::<Vec<_>>();
        names.sort();
        assert_eq!(names, expected, "{path}: {from}");
        assert!(changes.removed.is_empty());
    }
    let mut reduced = files(42);
    reduced[0].1 = reduced[0].1.replace("['app', 'other']", "['app']");
    let changes = capture(reduced).changes_since(&initial);
    assert!(changes.rebuild.is_empty());
    assert_eq!(
        changes
            .removed
            .iter()
            .map(|family| family.name())
            .collect::<Vec<_>>(),
        ["other"]
    );
    engine.set_max_semantic_diagnostics(23);
    let changes = capture(files(42)).changes_since(&initial);
    assert_eq!(
        changes.rebuild.len(),
        3,
        "environment changes affect every selected root"
    );
    assert!(changes.removed.is_empty());
    let cancelled = CancellationToken::default();
    cancelled.cancel();
    let project = ProjectInputs::load(
        &virtual_inputs(42),
        "app",
        &Default::default(),
        &Default::default(),
    )
    .unwrap();
    assert!(matches!(
        PackageBuildSet::capture(&engine, &project, &cancelled),
        Err(EmbeddingError::Cancelled)
    ));
}

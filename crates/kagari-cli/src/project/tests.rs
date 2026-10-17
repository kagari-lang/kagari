use crate::{
    args::{ProjectAction, ProjectArgs},
    project::{locate, run_from},
    runtime_engine,
};
use kagari::{BytecodeArtifact, program::PreparedProgram};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    process,
    time::{SystemTime, UNIX_EPOCH},
};

fn fixture() -> PathBuf {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/cli-projects")
        .join(format!(
            "{}-{}",
            process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    for (name, text) in [
        (
            "Kagari.toml",
            "[workspace]\nnamespace = 'tests/cli'\nmembers = ['app', 'tool']\ndefault-members = ['app']",
        ),
        (
            "app/Kagari.toml",
            "[package]\nname = 'app'\nversion = '1.0.0'\n[dependencies]\nrules = { path = '../rules' }",
        ),
        (
            "app/src/lib.kgr",
            "pub fn answer() -> i32 { rules::value() }\nfn main() -> i32 { answer() }",
        ),
        (
            "rules/Kagari.toml",
            "[package]\nname = 'rules'\nversion = '1.0.0'",
        ),
        ("rules/src/lib.kgr", "pub fn value() -> i32 { 42 }"),
        (
            "tool/Kagari.toml",
            "[package]\nname = 'tool'\nversion = '1.0.0'",
        ),
        ("tool/src/lib.kgr", "pub fn tool() -> i32 { true }"),
    ] {
        let path = directory.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    directory.canonicalize().unwrap()
}

fn artifacts(directory: &Path) -> BTreeSet<PathBuf> {
    fs::read_dir(directory.join("target/kagari/programs"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect()
}

#[test]
fn project_builds_select_roots_and_store_independent_artifacts() {
    let directory = fixture();
    // The unselected tool's bad body is irrelevant to this check.
    run_from(ProjectAction::Check, Default::default(), false, &directory).unwrap();
    let lock = fs::read(directory.join("Kagari.lock")).unwrap();
    assert!(!directory.join("target/kagari/programs").exists());
    let all = ProjectArgs {
        workspace: true,
        ..Default::default()
    };
    let error = run_from(ProjectAction::Run, all.clone(), false, &directory).unwrap_err();
    assert_eq!(error.exit_code(), 2);
    assert!(error.to_string().contains("exactly one"));
    assert_eq!(fs::read(directory.join("Kagari.lock")).unwrap(), lock);
    let tool = ProjectArgs {
        packages: vec!["tool".into()],
        ..Default::default()
    };
    assert!(
        run_from(ProjectAction::Check, tool, false, &directory)
            .unwrap_err()
            .to_string()
            .contains("diagnostics")
    );
    fs::write(
        directory.join("tool/src/lib.kgr"),
        "pub fn tool() -> i32 { 19 }",
    )
    .unwrap();
    run_from(ProjectAction::Build, all, false, &directory).unwrap();
    let previous = artifacts(&directory);
    assert_eq!(
        previous.len(),
        3,
        "each selected root has a separate Program"
    );
    // CLI artifacts carry host::log even when the package does not call it.
    let engine = runtime_engine().unwrap();
    let mut roots = BTreeSet::new();
    for path in &previous {
        let artifact = BytecodeArtifact::from_bytes(&fs::read(path).unwrap()).unwrap();
        assert!(roots.insert(artifact.records().header.module_identity.clone()));
        let program =
            PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap();
        let mut runtime = engine.runtime(Default::default());
        let loaded = runtime.load_program(&program, Default::default()).unwrap();
        let entries = ["answer", "tool", "value"]
            .into_iter()
            .filter(|entry| loaded.exported_function(entry).is_some())
            .collect::<Vec<_>>();
        assert_eq!(
            entries.len(),
            1,
            "dependency exports do not leak into the selected root"
        );
        let expected = if entries[0] == "tool" { 19 } else { 42 };
        assert_eq!(
            runtime
                .execute_typed::<(), i32>(&loaded, entries[0], (), &Default::default())
                .unwrap(),
            expected
        );
    }
    run_from(
        ProjectAction::Run,
        Default::default(),
        false,
        &directory.join("app/src"),
    )
    .unwrap();
    assert_eq!(
        artifacts(&directory),
        previous,
        "an unchanged build reuses the complete output"
    );
    // A local body edit is permitted by the unchanged shared lock. Explicit run
    // uses the member's private root entry and stores a new complete product.
    fs::write(
        directory.join("rules/src/lib.kgr"),
        "pub fn value() -> i32 { 43 }",
    )
    .unwrap();
    run_from(
        ProjectAction::Run,
        ProjectArgs {
            locked: true,
            offline: true,
            ..Default::default()
        },
        false,
        &directory.join("app/src"),
    )
    .unwrap();
    assert_eq!(fs::read(directory.join("Kagari.lock")).unwrap(), lock);
    let current = artifacts(&directory);
    assert_eq!(current.len(), 4);
    assert!(previous.is_subset(&current));
    let new_path = current.difference(&previous).next().unwrap();
    let artifact = BytecodeArtifact::from_bytes(&fs::read(new_path).unwrap()).unwrap();
    let program =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let mut runtime = engine.runtime(Default::default());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    assert_eq!(
        runtime
            .execute_typed::<(), i32>(&loaded, "answer", (), &Default::default())
            .unwrap(),
        43
    );
    assert!(!directory.join("app/Kagari.lock").exists());
    assert!(!directory.join("app/src/target").exists());
}

#[test]
fn project_discovery_bounds_and_locked_failures_precede_compilation() {
    let directory = fixture();
    let args = ProjectArgs {
        locked: true,
        ..Default::default()
    };
    assert!(
        run_from(ProjectAction::Check, args, false, &directory)
            .unwrap_err()
            .to_string()
            .contains("existing lock")
    );
    assert!(!directory.join("Kagari.lock").exists());
    assert!(!directory.join("target").exists());
    let args = ProjectArgs {
        manifest: Some(directory.join("app/Kagari.toml")),
        input_root: Some(directory.clone()),
        ..Default::default()
    };
    let (input, invoking) = locate(&args, Path::new(".")).unwrap();
    assert_eq!(input.root(), directory);
    assert_eq!(invoking, "app");
    let outside = ProjectArgs {
        input_root: Some(directory.join("tool")),
        ..args.clone()
    };
    assert!(
        locate(&outside, Path::new("."))
            .unwrap_err()
            .to_string()
            .contains("outside --input-root")
    );
    let unknown = ProjectArgs {
        packages: vec!["missing".into()],
        ..args
    };
    assert!(
        run_from(ProjectAction::Build, unknown, false, &directory)
            .unwrap_err()
            .to_string()
            .contains("unknown workspace package")
    );
    assert!(!directory.join("Kagari.lock").exists());
}

#[test]
fn check_reports_independent_roots_and_preserves_lock_on_partial_acquisition() {
    use crate::checking::project_report;

    let directory = fixture();
    let all = ProjectArgs {
        workspace: true,
        ..Default::default()
    };
    fs::write(directory.join("app/src/lib.kgr"), "mod hidden; fn broken(").unwrap();
    let report = project_report(&all, &directory);
    assert_eq!(report.summary.incomplete, 1);
    assert_eq!(
        report.summary.failed, 1,
        "tool's type error is still checked"
    );
    assert_eq!(
        report.summary.succeeded, 1,
        "independent rules root is checked"
    );
    assert!(!directory.join("Kagari.lock").exists());
    fs::write(
        directory.join("app/src/lib.kgr"),
        "pub fn answer() -> i32 { rules::value() }",
    )
    .unwrap();
    let report = project_report(&all, &directory);
    assert_eq!(report.summary.incomplete, 0);
    assert_eq!(report.summary.failed, 1);
    let lock = fs::read(directory.join("Kagari.lock")).unwrap();
    fs::remove_file(directory.join("app/src/lib.kgr")).unwrap();
    let report = project_report(&all, &directory);
    assert_eq!(report.summary.incomplete, 1);
    assert_eq!(report.summary.failed, 1);
    assert_eq!(report.summary.succeeded, 1);
    assert_eq!(fs::read(directory.join("Kagari.lock")).unwrap(), lock);
}

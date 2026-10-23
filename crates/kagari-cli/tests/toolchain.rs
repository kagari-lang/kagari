//! Actual command/output contracts; semantic rule cases live in kagari-lint.

use kagari::{
    context::ExecutionContext,
    engine::{KagariEngine, source::ArtifactOptions},
    error::EmbeddingError,
    program::PreparedProgram,
    runtime::LoadOptions,
};
use kagari_bytecode::artifact::ArtifactCompatibility;
use kagari_package::{
    filesystem::FilesystemSources,
    project::{ProjectInputs, ProjectOptions},
    snapshot::ProgramInputs,
    workspace::MemberSelection,
};
use kagari_runtime::{module::entry::ProgramEntry, value::Value};
use serde_json::{Value as JsonValue, from_slice};
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Project(PathBuf);

impl Project {
    fn new() -> Self {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/toolchain-contracts")
            .join(format!(
                "{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
        fs::create_dir_all(&root).unwrap();
        Self(root.canonicalize().unwrap())
    }

    fn write(&self, path: &str, text: &str) {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_kg"))
            .current_dir(&self.0)
            .args(args)
            .output()
            .unwrap()
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn records(output: &Output) -> Vec<JsonValue> {
    output
        .stdout
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| from_slice(line).unwrap())
        .collect()
}

#[test]
fn lint_cli_policy_required_errors_and_rule_help() {
    let project = Project::new();
    // Help needs neither a manifest nor provider/declaration-cache preparation.
    let list = project.run(&["lint", "--list-rules"]);
    assert!(list.status.success());
    assert_eq!(String::from_utf8(list.stdout).unwrap().lines().count(), 6);
    let explain = project.run(&["lint", "--explain", "unused_binding"]);
    assert!(explain.status.success());
    assert!(
        String::from_utf8(explain.stdout)
            .unwrap()
            .contains("effects")
    );
    assert!(!project.0.join("target").exists());
    assert_eq!(
        project.run(&["lint", "--explain", "missing"]).status.code(),
        Some(2)
    );
    assert_eq!(
        project
            .run(&["check", "-A", "unused_binding"])
            .status
            .code(),
        Some(2)
    );
    project.write("script.kgr", "fn good(){val unused=1;}");
    let warn = project.run(&["lint", "script.kgr", "--message-format", "json"]);
    assert!(warn.status.success(), "{warn:?}");
    let json = records(&warn);
    assert_eq!(json[0]["code"], "unused_binding");
    assert_eq!(json[0]["severity"], "warning");
    assert_eq!(json[0]["location"]["byte_start"], 14);
    assert_eq!(json.last().unwrap()["succeeded"], 1);
    let deny = project.run(&[
        "lint",
        "script.kgr",
        "-D",
        "warnings",
        "--message-format",
        "json",
    ]);
    assert_eq!(deny.status.code(), Some(1));
    assert_eq!(records(&deny)[0]["severity"], "error");
    assert!(
        project
            .run(&[
                "lint",
                "script.kgr",
                "-A",
                "unused_binding",
                "-D",
                "warnings"
            ])
            .status
            .success()
    );
    project.write(
        "script.kgr",
        "fn broken()->i32{true} fn good(){val unused=1;}",
    );
    let failure = project.run(&[
        "lint",
        "script.kgr",
        "-A",
        "unused_binding",
        "--message-format",
        "json",
    ]);
    assert_eq!(failure.status.code(), Some(1));
    assert!(
        records(&failure)
            .iter()
            .any(|record| record["severity"] == "error")
    );
    assert!(
        !records(&failure)
            .iter()
            .any(|record| record["code"] == "unused_binding")
    );
}

#[test]
fn lint_project_scope_and_configuration_precedence() {
    let project = Project::new();
    project.write(
        "Kagari.toml",
        "[package]\nname='app'\nnamespace='tests/toolchain'\nversion='1.0.0'\n[dependencies]\ndep={path='dep'}",
    );
    project.write(
        "src/lib.kgr",
        r#"#[tool::warn("unused_parameter")] fn good(value:i32){val unused=dep::answer();}"#,
    );
    project.write(
        "dep/Kagari.toml",
        "[package]\nname='dep'\nnamespace='tests/dependency'\nversion='1.0.0'",
    );
    project.write("dep/src/lib.kgr", "pub fn answer()->i32{val not_used=1;42}");
    project.write(
        "Kagari.tools.toml",
        "[lint]\nunused_parameter='allow'\nunused_binding='allow'",
    );
    let output = project.run(&["lint", "--message-format", "json"]);
    assert!(output.status.success(), "{output:?}");
    let json = records(&output);
    assert_eq!(json.len(), 2, "{json:?}");
    assert_eq!(json[0]["code"], "unused_parameter");
    assert!(project.0.join("Kagari.lock").is_file());
    let override_level =
        project.run(&["lint", "-D", "unused_parameter", "--message-format", "json"]);
    assert_eq!(override_level.status.code(), Some(1));
    assert_eq!(records(&override_level)[0]["severity"], "error");
    // Required dependency errors remain visible even when every optional rule is allowed.
    project.write("dep/src/lib.kgr", "pub fn answer()->i32{true}");
    assert_eq!(
        project
            .run(&["lint", "-A", "unused_parameter"])
            .status
            .code(),
        Some(1)
    );
    project.write("Kagari.tools.toml", "[lint]\nunknown='allow'");
    assert_eq!(project.run(&["lint"]).status.code(), Some(2));
}

#[test]
fn workspace_tools_preserve_semantics_and_agree_with_direct_sdk_checking() {
    let project = Project::new();
    project.write(
        "Kagari.toml",
        "[workspace]\nnamespace='tests/tools'\nmembers=['app','broken']\ndefault-members=['app']",
    );
    project.write("app/Kagari.toml", "[package]\nname='app'\nversion='1.0.0'");
    project.write(
        "broken/Kagari.toml",
        "[package]\nname='broken'\nversion='1.0.0'",
    );
    project.write(
        "Kagari.tools.toml",
        "[format]\nindent_width=2\nmax_width=60\n[lint]\nunused_binding='warn'",
    );
    let original = "mod child; fn main()->i32{val data=[0];val change=||{data[0]+=1;data[0]};val value=child::value();val _text=\"π {value}\";change()+change()+child::value()}";
    project.write("app/src/lib.kgr", original);
    project.write(
        "app/src/child.kgr",
        "pub fn value()->i32{40}// keep this comment\n",
    );
    project.write(
        "broken/src/lib.kgr",
        "fn broken()->i32{true} fn healthy(){val unused=1;}",
    );
    let input = FilesystemSources::new(&project.0).unwrap();
    let options = ProjectOptions {
        selection: MemberSelection::Workspace,
        ..Default::default()
    };
    let before = ProjectInputs::load(&input, "", &options, &Default::default()).unwrap();
    let engine = KagariEngine::builder().unwrap().build().unwrap();
    assert_eq!(execute(&engine, &before.programs()["app"]), Value::I32(43));
    assert_eq!(
        project
            .run(&["fmt", "--workspace", "--check"])
            .status
            .code(),
        Some(1)
    );
    assert_eq!(
        fs::read_to_string(project.0.join("app/src/lib.kgr")).unwrap(),
        original
    );
    assert!(project.run(&["fmt", "--workspace"]).status.success());
    assert!(!project.0.join("Kagari.lock").exists());
    assert!(
        project
            .run(&["fmt", "--workspace", "--check"])
            .status
            .success()
    );
    let formatted = fs::read(project.0.join("app/src/lib.kgr")).unwrap();
    assert!(project.run(&["fmt", "--workspace"]).status.success());
    assert_eq!(
        fs::read(project.0.join("app/src/lib.kgr")).unwrap(),
        formatted
    );
    let after = ProjectInputs::load(&input, "", &options, &Default::default()).unwrap();
    assert_eq!(execute(&engine, &after.programs()["app"]), Value::I32(43));
    let cli = project.run(&["check", "--workspace", "--message-format", "json"]);
    assert_eq!(cli.status.code(), Some(1));
    let json = records(&cli);
    assert_eq!(json.last().unwrap()["succeeded"], 1);
    assert_eq!(json.last().unwrap()["failed"], 1);
    assert_eq!(json.last().unwrap()["incomplete"], 0);
    for (member, inputs) in after.programs() {
        let result = engine.compile_package(inputs, &Default::default());
        let errors: Vec<_> = json
            .iter()
            .filter(|record| record["context"] == *member && record["severity"] == "error")
            .collect();
        assert_eq!(result.is_ok(), errors.is_empty());
        if let Err(EmbeddingError::Diagnostics { diagnostics }) = result {
            for diagnostic in diagnostics {
                assert!(
                    errors
                        .iter()
                        .any(|record| record["code"] == diagnostic.code)
                );
            }
        }
    }
    let lint = project.run(&["lint", "--workspace", "--message-format", "json"]);
    assert_eq!(lint.status.code(), Some(1));
    assert!(
        records(&lint)
            .iter()
            .any(|record| record["code"] == "unused_binding" && record["context"] == "broken")
    );
    assert!(!project.0.join("target/kagari/programs").exists());
}

fn execute(engine: &KagariEngine, inputs: &ProgramInputs) -> Value {
    let checked = engine.compile_package(inputs, &Default::default()).unwrap();
    let artifact = engine
        .emit_artifact(&checked, ArtifactOptions::default())
        .unwrap();
    let context = ExecutionContext::default();
    let program = PreparedProgram::from_artifact(
        artifact,
        &ArtifactCompatibility::default(),
        &context.cancellation,
    )
    .unwrap();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime
        .load_program(&program, LoadOptions::default())
        .unwrap();
    runtime
        .execute(&loaded, ProgramEntry::Main, &[], &context)
        .unwrap()
        .return_value
        .value(runtime.runtime().gc())
        .unwrap()
}

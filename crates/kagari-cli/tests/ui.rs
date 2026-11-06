use crate::support::Project;
use kagari_source::source_database::normalize_source_name;
use rstest::rstest;
use std::{path::PathBuf, process::Output};

mod fixtures;
mod tests;
mod wire;

#[derive(Clone, Copy)]
struct Case {
    stem: &'static str,
    source: &'static str,
    args: &'static [&'static str],
    json: bool,
}

const CASES: [Case; 6] = [
    Case {
        stem: "unused-binding.warn-human",
        source: "unused-binding",
        args: &["lint", "script.kgr", "--message-format", "human"],
        json: false,
    },
    Case {
        stem: "unused-binding.warn-json",
        source: "unused-binding",
        args: &["lint", "script.kgr", "--message-format", "json"],
        json: true,
    },
    Case {
        stem: "unused-binding.deny-warnings-human",
        source: "unused-binding",
        args: &[
            "lint",
            "script.kgr",
            "-D",
            "warnings",
            "--message-format",
            "human",
        ],
        json: false,
    },
    Case {
        stem: "unused-binding.deny-warnings-json",
        source: "unused-binding",
        args: &[
            "lint",
            "script.kgr",
            "-D",
            "warnings",
            "--message-format",
            "json",
        ],
        json: true,
    },
    Case {
        stem: "required-error.human",
        source: "required-error",
        args: &[
            "lint",
            "script.kgr",
            "-A",
            "unused_binding",
            "--message-format",
            "human",
        ],
        json: false,
    },
    Case {
        stem: "required-error.json",
        source: "required-error",
        args: &[
            "lint",
            "script.kgr",
            "-A",
            "unused_binding",
            "--message-format",
            "json",
        ],
        json: true,
    },
];

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ui/lint")
}

fn compare(
    output: &Output,
    expected: &fixtures::Expected,
    case: &Case,
    root: &str,
) -> Result<(), String> {
    if output.status.code() != Some(expected.exit) {
        return Err(format!(
            "wrong process exit: {:?}, expected {}",
            output.status.code(),
            expected.exit
        ));
    }
    let stderr = if case.json {
        output.stderr.clone()
    } else {
        wire::human(&output.stderr, root)?
    };
    if stderr != expected.stderr {
        return Err(format!(
            "stderr differs\nactual: {:?}\nexpected: {:?}",
            String::from_utf8_lossy(&stderr),
            String::from_utf8_lossy(&expected.stderr)
        ));
    }
    if case.json {
        wire::compare(
            &output.stdout,
            &expected.stdout,
            root,
            &expected.source,
            expected.exit,
        )?;
    } else if output.stdout != expected.stdout {
        return Err("stdout differs".into());
    }
    Ok(())
}

#[rstest]
#[case::warn_human(&CASES[0])]
#[case::warn_json(&CASES[1])]
#[case::deny_warnings_human(&CASES[2])]
#[case::deny_warnings_json(&CASES[3])]
#[case::required_error_human(&CASES[4])]
#[case::required_error_json(&CASES[5])]
fn diagnostics(#[case] case: &Case) {
    let expected = fixtures::load(&fixture_root(), case).unwrap();
    let project = Project::new();
    project.write("script.kgr", &expected.source);
    let output = project.run(case.args);
    let physical = project.0.to_str().unwrap();
    // Windows canonicalize adds a verbatim prefix; the child's cwd is an ordinary drive path.
    let physical = physical.strip_prefix(r"\\?\").unwrap_or(physical);
    let root = normalize_source_name(physical).unwrap();
    compare(&output, &expected, case, &root).unwrap();
}

#[test]
fn fixture_inventory() {
    fixtures::inventory(&fixture_root()).unwrap();
}

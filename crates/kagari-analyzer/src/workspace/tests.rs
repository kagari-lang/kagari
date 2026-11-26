use super::*;
use crate::AnalyzerQueries;
use kagari_common::{
    identity::{ModuleIdentity, PackageId},
    span::Span,
};
use kagari_hir::imports::environment::ModuleScope;
use kagari_source::package::PackageScope;
use std::hash::{BuildHasher, BuildHasherDefault, DefaultHasher};

const USER_PATH: &str = "tests/fixtures/score.kgr";
const DISK: &str = include_str!("../../tests/fixtures/score.kgr");
const KEY: &str = "kagari://native/simulation/api";
const NUMBER: &str = "pub native fn number() -> i32;\n";
const ADDED: &str = "pub native fn added() -> i32;\n";

fn export(text: &str) -> CapturedSource {
    let hash = BuildHasherDefault::<DefaultHasher>::default().hash_one(text);
    CapturedSource {
        key: KEY.into(),
        display_name: format!("/cache/{hash:016x}.kgr"),
        module: ModuleIdentity {
            package: PackageId("simulation".into()),
            path: vec!["api".into()],
        },
        text: text.into(),
    }
}

fn imports(client: &ModuleIdentity, selection: &str) -> ImportEnvironment {
    let scope = ModuleScope {
        packages: PackageScope {
            dependencies: [("game".into(), PackageId("simulation".into()))].into(),
            selection: selection.into(),
        },
        ..Default::default()
    };
    ImportEnvironment {
        modules: [(client.clone(), scope)].into(),
    }
}

#[test]
fn unsaved_buffer_navigates_to_each_snapshots_own_export_then_close_restores_disk() {
    let sources = SourceDatabase::new(env!("CARGO_MANIFEST_DIR")).unwrap();
    let client = ModuleIdentity::single_file(sources.source_name(USER_PATH).unwrap());
    let mut workspace = EditorWorkspace::new(sources).unwrap();
    let cancel = CancellationToken::default();
    let first = export(NUMBER);
    let first_path = first.display_name.clone();
    let first_imports = imports(&client, "first");
    workspace
        .replace_generated(vec![first], first_imports.clone(), &cancel)
        .unwrap();
    let user = workspace
        .open(USER_PATH, DISK.replace("score()", "draft()"))
        .unwrap();
    let edited = DISK.replace("score()", "unsaved_score()");
    workspace.change(USER_PATH, edited.clone()).unwrap();
    let provider = workspace.sources().file_id(KEY).unwrap();
    let second = export(&format!("{ADDED}{NUMBER}"));
    let second_path = second.display_name.clone();
    assert_ne!(first_path, second_path);
    let second_imports = imports(&client, "second");

    // Losing the paired scopes on a failed delivery would break `game::api` lookup.
    let cancelled = CancellationToken::default();
    cancelled.cancel();
    assert!(
        workspace
            .replace_generated(vec![], ImportEnvironment::default(), &cancelled)
            .is_err()
    );
    let old = workspace.analyze(&cancel).unwrap();
    workspace
        .replace_generated(vec![second], second_imports.clone(), &cancel)
        .unwrap();
    let new = workspace.analyze(&cancel).unwrap();
    workspace.close(USER_PATH).unwrap();
    assert_eq!(
        workspace.sources().snapshot().file(user).unwrap().text(),
        DISK
    );

    for (snapshot, path, prefix, expected_imports) in [
        (&old, first_path, 0, first_imports),
        (&new, second_path, ADDED.len(), second_imports),
    ] {
        assert!(snapshot.inputs().native_modules().is_empty());
        assert_eq!(snapshot.inputs().import_environment(), &expected_imports);
        assert_eq!(snapshot.source(user).unwrap().text(), edited);
        snapshot.check_program(user, &cancel).unwrap();
        let target = snapshot
            .queries()
            .definition_at(user, edited.find("number()").unwrap())
            .unwrap();
        let generated = snapshot.source(provider).unwrap();
        let start = prefix + "pub native fn ".len();
        assert_eq!(generated.name(), path);
        assert_eq!(
            target.source_location().unwrap(),
            generated
                .span(Span::new(start, start + "number".len()))
                .unwrap()
        );
    }
}

use crate::{
    graph::{GraphLimits, ResolvedGraph},
    host::HostPackage,
    identity::PackageFamily,
    lock::ResolutionOptions,
    modules::{ModuleLimits, VirtualSources},
    snapshot::ProgramInputs,
    workspace::{WorkspaceInputs, WorkspaceLimits},
};
use kagari_common::cancellation::CancellationToken;
use kagari_hir::analysis::AnalysisDatabase;
use kagari_source::source_database::SourceDatabase;
use std::{collections::BTreeMap, sync::Arc};

fn sources(files: &[(&str, &str)]) -> VirtualSources {
    VirtualSources::new(
        files
            .iter()
            .map(|(name, text)| (name.to_string(), Arc::from(*text))),
        &CancellationToken::default(),
    )
    .unwrap()
}

fn capture(files: &[(&str, &str)], hosts: BTreeMap<String, Arc<HostPackage>>) -> ProgramInputs {
    let input = sources(files);
    let cancel = CancellationToken::default();
    let workspace = WorkspaceInputs::load(
        &input,
        "",
        None,
        WorkspaceLimits::default(),
        Default::default(),
        &cancel,
    )
    .unwrap();
    let graph = ResolvedGraph::resolve(
        &input,
        &workspace,
        &hosts,
        None,
        ResolutionOptions::default(),
        GraphLimits::default(),
        &cancel,
    )
    .unwrap();
    graph
        .capture(
            &input,
            &graph.members()[""],
            ModuleLimits::default(),
            &cancel,
        )
        .unwrap()
}

fn rules(namespace: &str, body: &str) -> Arc<HostPackage> {
    Arc::new(
        HostPackage::new(
            PackageFamily::new(namespace, "", "rules").unwrap(),
            "rev1",
            "[package]\nname = 'rules'\nversion = '1.0.0'",
            sources(&[("src/lib.kgr", body)]),
            &CancellationToken::default(),
        )
        .unwrap(),
    )
}

#[test]
fn each_package_resolves_its_own_alias_and_cannot_import_transitive_dependencies() {
    let hosts: BTreeMap<_, _> = [
        (
            "integer".into(),
            rules("example/integer", "pub fn value() -> i32 { 7 }"),
        ),
        (
            "boolean".into(),
            rules("example/boolean", "pub fn value() -> bool { true }"),
        ),
    ]
    .into();
    for (body, accepted) in [
        ("pub fn run() -> i32 { left::value() }", true),
        ("pub fn run() -> i32 { rules::value() }", false),
    ] {
        let input = capture(
            &[
                (
                    "Kagari.toml",
                    "[package]\nname = 'app'\nversion = '1.0.0'\n[workspace]\nnamespace = 'example/app'\n[dependencies]\nleft = { path = 'left' }\nright = { path = 'right' }",
                ),
                ("src/lib.kgr", body),
                (
                    "left/Kagari.toml",
                    "[package]\nname = 'left'\nversion = '1.0.0'\n[dependencies]\nrules = { host = 'integer' }",
                ),
                (
                    "left/src/lib.kgr",
                    "pub fn value() -> i32 { rules::value() }",
                ),
                (
                    "right/Kagari.toml",
                    "[package]\nname = 'right'\nversion = '1.0.0'\n[dependencies]\nrules = { host = 'boolean' }",
                ),
                (
                    "right/src/lib.kgr",
                    "pub fn value() -> bool { rules::value() }",
                ),
            ],
            hosts.clone(),
        );
        let mut database = SourceDatabase::default();
        let source = input
            .source_snapshot(&mut database, &Default::default())
            .unwrap();
        let root = source
            .files()
            .find(|file| {
                file.module_identity().package == input.root().family.package_id()
                    && file.module_identity().path.is_empty()
            })
            .unwrap()
            .id();
        let analysis = AnalysisDatabase::default()
            .snapshot(source, &Default::default())
            .unwrap();
        let result = analysis.check_program(root, &Default::default());
        assert_eq!(
            result.is_ok(),
            accepted,
            "{:?}",
            analysis.file(root).unwrap().result().diagnostics()
        );
    }
}

#[test]
fn declared_unused_dependency_bodies_and_all_module_bodies_are_checked() {
    for (root, child, dependency) in [
        ("fn main() {}", "", "fn broken() -> i32 { true }"),
        ("mod child; fn main() {}", "fn broken() -> i32 { true }", ""),
    ] {
        let input = capture(
            &[
                (
                    "Kagari.toml",
                    "[package]\nname = 'app'\nversion = '1.0.0'\n[workspace]\nnamespace = 'example/app'\n[dependencies]\ndep = { path = 'dep' }",
                ),
                ("src/lib.kgr", root),
                ("src/child.kgr", child),
                (
                    "dep/Kagari.toml",
                    "[package]\nname = 'dep'\nversion = '1.0.0'",
                ),
                ("dep/src/lib.kgr", dependency),
            ],
            BTreeMap::new(),
        );
        let source = input
            .source_snapshot(&mut SourceDatabase::default(), &Default::default())
            .unwrap();
        let root = source
            .files()
            .find(|file| file.module_identity().package == input.root().family.package_id())
            .unwrap()
            .id();
        let analysis = AnalysisDatabase::default()
            .snapshot(source, &Default::default())
            .unwrap();
        assert!(analysis.check_program(root, &Default::default()).is_err());
    }
}

#[test]
fn unnamed_root_paths_and_package_visibility_work_across_external_and_inline_modules() {
    let input = capture(
        &[
            (
                "Kagari.toml",
                "[package]\nname = 'app'\nversion = '1.0.0'\nnamespace = 'example/app'",
            ),
            (
                "src/lib.kgr",
                "pub(crate) fn base() -> i32 { 1 } mod a; mod b { pub(crate) fn call() -> i32 { super::base() } } pub fn run() -> i32 { self::a::call() }",
            ),
            (
                "src/a.kgr",
                "use crate::b::call as sibling; pub(crate) fn call() -> i32 { sibling() }",
            ),
        ],
        BTreeMap::new(),
    );
    let source = input
        .source_snapshot(&mut SourceDatabase::default(), &Default::default())
        .unwrap();
    let root = source
        .files()
        .find(|file| file.module_identity().path.is_empty())
        .unwrap()
        .id();
    let analysis = AnalysisDatabase::default()
        .snapshot(source, &Default::default())
        .unwrap();
    let errors = analysis
        .module_graph()
        .modules()
        .flat_map(|(_, node)| {
            analysis
                .file(node.file)
                .unwrap()
                .result()
                .diagnostics()
                .to_vec()
        })
        .collect::<Vec<_>>();
    assert!(errors.is_empty(), "{errors:?}");
    analysis.check_program(root, &Default::default()).unwrap();
}

#[test]
fn retained_snapshots_keep_old_signatures_when_dependency_text_changes_without_root_edits() {
    let mut database = SourceDatabase::default();
    let mut analysis_database = AnalysisDatabase::default();
    let mut old = None;
    let mut root_revision = None;
    for (body, accepted) in [
        ("pub fn value() -> i32 { 1 }", true),
        ("pub fn value() -> bool { true }", false),
    ] {
        let input = capture(
            &[
                (
                    "Kagari.toml",
                    "[package]\nname = 'app'\nversion = '1.0.0'\n[workspace]\nnamespace = 'example/app'\n[dependencies]\ndep = { path = 'dep' }",
                ),
                ("src/lib.kgr", "pub fn run() -> i32 { dep::value() }"),
                (
                    "dep/Kagari.toml",
                    "[package]\nname = 'dep'\nversion = '1.0.0'",
                ),
                ("dep/src/lib.kgr", body),
            ],
            BTreeMap::new(),
        );
        let source = input
            .source_snapshot(&mut database, &Default::default())
            .unwrap();
        let root = source
            .files()
            .find(|file| file.module_identity().package == input.root().family.package_id())
            .unwrap();
        if let Some(previous) = root_revision {
            assert_eq!(root.revision(), previous);
        }
        root_revision = Some(root.revision());
        let root = root.id();
        let snapshot = analysis_database
            .snapshot(source, &Default::default())
            .unwrap();
        assert_eq!(
            snapshot.check_program(root, &Default::default()).is_ok(),
            accepted,
            "{:?}",
            snapshot.file(root).unwrap().result().diagnostics()
        );
        if accepted {
            old = Some((snapshot, root));
        }
    }
    let (old, root) = old.unwrap();
    old.check_program(root, &Default::default()).unwrap();
}

#[test]
fn alias_only_replacement_removes_old_package_inputs_without_mutating_retained_snapshots() {
    let mut database = SourceDatabase::default();
    let mut analysis = AnalysisDatabase::default();
    let mut previous = None;
    for (key, body, accepted) in [
        ("integer", "pub fn value() -> i32 { 1 }", true),
        ("boolean", "pub fn value() -> bool { true }", false),
    ] {
        let input = capture(
            &[
                (
                    "Kagari.toml",
                    &format!(
                        "[package]\nname = 'app'\nversion = '1.0.0'\nnamespace = 'example/app'\n[dependencies]\ndep = {{ package = 'rules', host = '{key}' }}"
                    ),
                ),
                ("src/lib.kgr", "pub fn run() -> i32 { dep::value() }"),
            ],
            [(key.to_string(), rules(&format!("example/{key}"), body))].into(),
        );
        let source = input
            .source_snapshot(&mut database, &Default::default())
            .unwrap();
        assert_eq!(source.files().count(), 2);
        let root = source
            .files()
            .find(|file| file.module_identity().package == input.root().family.package_id())
            .unwrap()
            .id();
        let snapshot = analysis.snapshot(source, &Default::default()).unwrap();
        assert_eq!(
            snapshot.check_program(root, &Default::default()).is_ok(),
            accepted,
            "{:?}",
            snapshot.file(root).unwrap().result().diagnostics()
        );
        if accepted {
            previous = Some((snapshot, root));
        }
    }
    let (old, root) = previous.unwrap();
    old.check_program(root, &Default::default()).unwrap();
}

#[test]
fn implementation_only_dependencies_participate_in_selection_and_conflict_checks() {
    for (dependencies, accepted) in [
        (
            "model = { path = 'model' }\nextension = { path = 'extension' }",
            true,
        ),
        ("model = { path = 'model' }", false),
        (
            "model = { path = 'model' }\nextension = { path = 'extension' }\nconflict = { path = 'conflict' }",
            false,
        ),
    ] {
        let input = capture(
            &[
                (
                    "Kagari.toml",
                    &format!(
                        "[package]\nname = 'app'\nversion = '1.0.0'\n[workspace]\nnamespace = 'example/app'\nmembers = ['model', 'extension', 'conflict']\n[dependencies]\n{dependencies}"
                    ),
                ),
                (
                    "src/lib.kgr",
                    "use model::{Read, Data}; pub fn run() -> i32 { Data {}.read() }",
                ),
                (
                    "model/Kagari.toml",
                    "[package]\nname = 'model'\nversion = '1.0.0'",
                ),
                (
                    "model/src/lib.kgr",
                    "pub trait Read { fn read(self) -> i32; } pub struct Data {}",
                ),
                (
                    "extension/Kagari.toml",
                    "[package]\nname = 'extension'\nversion = '1.0.0'\n[dependencies]\nmodel = { path = '../model' }",
                ),
                (
                    "extension/src/lib.kgr",
                    "use model::{Read, Data}; impl Read for Data { fn read(self) -> i32 { 1 } }",
                ),
                (
                    "conflict/Kagari.toml",
                    "[package]\nname = 'conflict'\nversion = '1.0.0'\n[dependencies]\nmodel = { path = '../model' }",
                ),
                (
                    "conflict/src/lib.kgr",
                    "use model::{Read, Data}; impl Read for Data { fn read(self) -> i32 { 2 } }",
                ),
            ],
            BTreeMap::new(),
        );
        let source = input
            .source_snapshot(&mut SourceDatabase::default(), &Default::default())
            .unwrap();
        let root = source
            .files()
            .find(|file| file.module_identity().package == input.root().family.package_id())
            .unwrap()
            .id();
        let snapshot = AnalysisDatabase::default()
            .snapshot(source, &Default::default())
            .unwrap();
        assert_eq!(
            snapshot.check_program(root, &Default::default()).is_ok(),
            accepted,
            "{:?}",
            snapshot.file(root).unwrap().result().diagnostics()
        );
    }
}

#[test]
fn public_export_graph_preserves_aliases_cycles_and_the_separate_private_entry() {
    let input = capture(
        &[
            (
                "Kagari.toml",
                "[package]\nname = 'app'\nversion = '1.0.0'\nnamespace = 'example/exports'",
            ),
            (
                "src/lib.kgr",
                "mod hidden { pub fn calculate() -> i32 { 7 } } pub use crate::hidden::calculate as damage; pub mod api { pub fn tick() -> bool { true } } pub fn api() -> i32 { 0 } pub use crate as again; fn main() {} fn secret() {} pub(crate) fn sibling() {} pub fn generic<T>(value: T) -> T { value } pub fn concrete() -> i32 { generic(3) }",
            ),
        ],
        BTreeMap::new(),
    );
    let source = input
        .source_snapshot(&mut SourceDatabase::default(), &Default::default())
        .unwrap();
    let root = source
        .files()
        .find(|file| file.module_identity().path.is_empty())
        .unwrap()
        .id();
    let snapshot = AnalysisDatabase::default()
        .snapshot(source, &Default::default())
        .unwrap();
    let program = snapshot.check_program(root, &Default::default()).unwrap();
    let identity = program.root().lowered.source.module_identity();
    let exports = program.exports();
    let damage = exports.function(identity, "damage").unwrap();
    assert_eq!(program.source_function(damage).unwrap().1.name, "calculate");
    assert_eq!(
        exports.function(identity, "again::again::damage"),
        Some(damage)
    );
    assert!(exports.function(identity, "api::tick").is_some());
    assert!(exports.function(identity, "api").is_some());
    assert!(exports.function(identity, "concrete").is_some());
    for private in [
        "hidden::calculate",
        "main",
        "secret",
        "sibling",
        "generic",
        "api::",
        "::damage",
    ] {
        assert!(exports.function(identity, private).is_none(), "{private}");
    }
    let oversized = format!("{}damage", "again::".repeat(64));
    assert!(exports.function(identity, &oversized).is_none());
    assert_eq!(
        program
            .source_function(exports.entry().unwrap())
            .unwrap()
            .1
            .name,
        "main"
    );
}

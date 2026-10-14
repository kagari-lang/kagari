mod support;

use kagari_common::cancellation::CancellationToken;
use kagari_package::{
    filesystem::FilesystemSources,
    graph::{GraphLimits, ResolvedGraph},
    host::HostPackage,
    identity::PackageFamily,
    lock::{Lockfile, ResolutionOptions},
    modules::{ModuleLimits, VirtualSources},
    workspace::{WorkspaceInputs, WorkspaceLimits},
};
use std::{collections::BTreeMap, fs, sync::Arc};
use support::{link_directory, scratch};

fn input(files: &[(&str, &str)]) -> VirtualSources {
    VirtualSources::new(
        files
            .iter()
            .map(|(name, text)| (name.to_string(), Arc::from(*text))),
        &CancellationToken::default(),
    )
    .unwrap()
}

fn workspace(input: &VirtualSources) -> WorkspaceInputs {
    WorkspaceInputs::load(
        input,
        "",
        None,
        WorkspaceLimits::default(),
        &CancellationToken::default(),
    )
    .unwrap()
}

fn local_sources(shared_body: &str) -> VirtualSources {
    input(&[
        (
            "Kagari.toml",
            "[workspace]\nnamespace = 'example/game'\nmembers = ['game', 'tool']",
        ),
        (
            "game/Kagari.toml",
            "[package]\nname = 'game'\nversion = '1.0.0'\n[dependencies]\nshared = { path = '../shared', version = '1' }",
        ),
        ("game/src/lib.kgr", "pub fn run() {}"),
        (
            "tool/Kagari.toml",
            "[package]\nname = 'tool'\nversion = '1.0.0'",
        ),
        ("tool/src/lib.kgr", "invalid syntax in unrelated root"),
        (
            "shared/Kagari.toml",
            "[package]\nname = 'shared'\nversion = '1.0.0'",
        ),
        ("shared/src/lib.kgr", shared_body),
    ])
}

fn resolve(
    input: &VirtualSources,
    hosts: &BTreeMap<String, Arc<HostPackage>>,
    previous: Option<&Lockfile>,
    options: ResolutionOptions,
) -> ResolvedGraph {
    ResolvedGraph::resolve(
        input,
        &workspace(input),
        hosts,
        previous,
        options,
        GraphLimits::default(),
        &CancellationToken::default(),
    )
    .unwrap()
}

fn host(namespace: &str, version: &str, revision: &str, body: &str) -> Arc<HostPackage> {
    Arc::new(
        HostPackage::new(
            PackageFamily::new(namespace, "", "rules").unwrap(),
            revision,
            &format!("[package]\nname = 'rules'\nversion = '{version}'"),
            input(&[("src/lib.kgr", body)]),
            &CancellationToken::default(),
        )
        .unwrap(),
    )
}

fn host_user(requirement: &str) -> VirtualSources {
    host_user_version(requirement, "1.0.0")
}

fn host_user_version(requirement: &str, version: &str) -> VirtualSources {
    input(&[
        (
            "Kagari.toml",
            &format!(
                "[package]\nname = 'app'\nversion = '{version}'\nnamespace = 'example/app'\n[dependencies]\nrules = {{ host = 'rules', version = '{requirement}' }}"
            ),
        ),
        ("src/lib.kgr", "pub fn run() {}"),
    ])
}

#[test]
fn whole_workspace_locks_all_members_but_capture_reads_only_the_declared_package_closure() {
    let sources = local_sources("pub fn count() -> i32 { 1 }");
    let graph = resolve(
        &sources,
        &BTreeMap::new(),
        None,
        ResolutionOptions::default(),
    );
    assert_eq!(graph.members().len(), 3);
    assert_eq!(graph.packages().len(), 3);
    let cancel = CancellationToken::default();
    let lock = Lockfile::from_graph(&graph);
    assert_eq!(
        Lockfile::parse("Kagari.lock", &lock.encode().unwrap(), &cancel).unwrap(),
        lock
    );
    let captured = graph
        .capture(
            &sources,
            &graph.members()["game"],
            ModuleLimits::default(),
            &cancel,
        )
        .unwrap();
    assert_eq!(captured.packages().len(), 2);
    assert!(
        captured
            .packages()
            .keys()
            .any(|selection| selection.family.name() == "shared")
    );
    assert!(
        graph
            .capture(
                &sources,
                &graph.members()["tool"],
                ModuleLimits::default(),
                &cancel
            )
            .is_err()
    );
}

#[test]
fn local_body_edits_change_snapshots_without_changing_the_lock_or_retained_inputs() {
    let sources = local_sources("pub fn count() -> i32 { 1 }");
    let graph = resolve(
        &sources,
        &BTreeMap::new(),
        None,
        ResolutionOptions::default(),
    );
    let cancel = CancellationToken::default();
    let before = graph
        .capture(
            &sources,
            &graph.members()["game"],
            ModuleLimits::default(),
            &cancel,
        )
        .unwrap();
    let lock = Lockfile::from_graph(&graph);
    let edited = local_sources("pub fn count() -> i32 { 2 }");
    let updated = resolve(
        &edited,
        &BTreeMap::new(),
        Some(&lock),
        ResolutionOptions {
            locked: true,
            offline: true,
        },
    );
    assert_eq!(Lockfile::from_graph(&updated), lock);
    let after = updated
        .capture(
            &edited,
            &updated.members()["game"],
            ModuleLimits::default(),
            &cancel,
        )
        .unwrap();
    assert_ne!(before.fingerprint(), after.fingerprint());
    let old_shared = before
        .packages()
        .iter()
        .find(|(selection, _)| selection.family.name() == "shared")
        .unwrap()
        .1;
    assert!(
        old_shared.tree().files["shared/src/lib.kgr"]
            .text
            .contains("{ 1 }")
    );
    let error = graph
        .capture(
            &sources,
            &graph.members()["game"],
            ModuleLimits {
                max_modules: 1,
                ..ModuleLimits::default()
            },
            &cancel,
        )
        .unwrap_err();
    assert!(error.to_string().contains("limit"));
}

#[test]
fn graph_rejects_dependency_cycles_name_version_mismatch_and_resource_exhaustion() {
    let cancel = CancellationToken::default();
    for (dependency, expected) in [
        ("app = { path = '.' }", "cycle"),
        ("different = { path = '.' }", "package/version"),
        ("app = { path = '.', version = '2' }", "package/version"),
    ] {
        let sources = input(&[(
            "Kagari.toml",
            &format!(
                "[package]\nname = 'app'\nversion = '1.0.0'\nnamespace = 'example/app'\n[dependencies]\n{dependency}"
            ),
        )]);
        let error = ResolvedGraph::resolve(
            &sources,
            &workspace(&sources),
            &BTreeMap::new(),
            None,
            ResolutionOptions::default(),
            GraphLimits::default(),
            &cancel,
        )
        .unwrap_err();
        assert!(error.to_string().contains(expected), "{error}");
    }
    let sources = local_sources("");
    for limits in [
        GraphLimits {
            max_packages: 1,
            ..GraphLimits::default()
        },
        GraphLimits {
            max_edges: 0,
            ..GraphLimits::default()
        },
    ] {
        assert!(
            ResolvedGraph::resolve(
                &sources,
                &workspace(&sources),
                &BTreeMap::new(),
                None,
                ResolutionOptions::default(),
                limits,
                &cancel
            )
            .unwrap_err()
            .to_string()
            .contains("limit")
        );
    }
}

#[test]
fn valid_host_lock_is_preserved_and_unavailable_exact_revisions_do_not_silently_upgrade() {
    let sources = host_user("*");
    let old = host(
        "example/rules",
        "1.0.0",
        "rev1",
        "pub fn value() -> i32 { 1 }",
    );
    let new = host(
        "example/rules",
        "2.0.0",
        "rev2",
        "pub fn value() -> i32 { 2 }",
    );
    let original = resolve(
        &sources,
        &[("rules".into(), old.clone())].into(),
        None,
        ResolutionOptions::default(),
    );
    let lock = Lockfile::from_graph(&original);
    let catalog = [
        ("rules".into(), new.clone()),
        ("retained-rev1".into(), old.clone()),
    ]
    .into();
    let preserved = resolve(
        &sources,
        &catalog,
        Some(&lock),
        ResolutionOptions::default(),
    );
    assert_eq!(Lockfile::from_graph(&preserved), lock);
    let local_version_change = host_user_version("*", "1.1.0");
    let updated_root = resolve(
        &local_version_change,
        &catalog,
        Some(&lock),
        ResolutionOptions::default(),
    );
    assert!(updated_root.packages().contains_key(old.selection()));
    assert!(!updated_root.packages().contains_key(new.selection()));
    let unavailable = [("rules".into(), new.clone())].into();
    let error = ResolvedGraph::resolve(
        &sources,
        &workspace(&sources),
        &unavailable,
        Some(&lock),
        ResolutionOptions::default(),
        GraphLimits::default(),
        &CancellationToken::default(),
    )
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("locked host input is unavailable"),
        "{error}"
    );
    let changed_requirement = host_user("2");
    let changed = resolve(
        &changed_requirement,
        &catalog,
        Some(&lock),
        ResolutionOptions {
            offline: true,
            locked: false,
        },
    );
    assert!(changed.packages().contains_key(new.selection()));
    assert!(!changed.packages().contains_key(old.selection()));
    let error = ResolvedGraph::resolve(
        &changed_requirement,
        &workspace(&changed_requirement),
        &catalog,
        Some(&lock),
        ResolutionOptions {
            locked: true,
            offline: false,
        },
        GraphLimits::default(),
        &CancellationToken::default(),
    )
    .unwrap_err();
    assert!(
        error.to_string().contains("unchanged whole-workspace lock"),
        "{error}"
    );
}

#[test]
fn immutable_host_revision_integrity_is_checked_against_catalog_and_lock() {
    let sources = host_user("1");
    let original = host(
        "example/rules",
        "1.0.0",
        "rev1",
        "pub fn value() -> i32 { 1 }",
    );
    let corrupt = host(
        "example/rules",
        "1.0.0",
        "rev1",
        "pub fn value() -> i32 { 2 }",
    );
    let graph = resolve(
        &sources,
        &[("rules".into(), original.clone())].into(),
        None,
        ResolutionOptions::default(),
    );
    let lock = Lockfile::from_graph(&graph);
    let cancel = CancellationToken::default();
    assert_eq!(
        Lockfile::parse("lock", &lock.encode().unwrap(), &cancel).unwrap(),
        lock
    );
    for (catalog, previous) in [
        ([("rules".into(), corrupt.clone())].into(), Some(&lock)),
        (
            [("rules".into(), original), ("duplicate".into(), corrupt)].into(),
            None,
        ),
    ] {
        let error = ResolvedGraph::resolve(
            &sources,
            &workspace(&sources),
            &catalog,
            previous,
            ResolutionOptions::default(),
            GraphLimits::default(),
            &cancel,
        )
        .unwrap_err();
        assert!(error.to_string().contains("content"), "{error}");
    }
}

#[test]
fn disjoint_programs_can_select_different_revisions_but_one_program_cannot() {
    let sources = input(&[
        (
            "Kagari.toml",
            "[workspace]\nnamespace = 'example/ws'\nmembers = ['a', 'b']",
        ),
        (
            "a/Kagari.toml",
            "[package]\nname = 'a'\nversion = '1.0.0'\n[dependencies]\nrules = { host = 'old' }",
        ),
        (
            "b/Kagari.toml",
            "[package]\nname = 'b'\nversion = '1.0.0'\n[dependencies]\nrules = { host = 'new' }",
        ),
        ("a/src/lib.kgr", "pub fn run() {}"),
        ("b/src/lib.kgr", "pub fn run() {}"),
    ]);
    let old = host(
        "example/rules",
        "1.0.0",
        "rev1",
        "pub fn value() -> i32 { 1 }",
    );
    let new = host(
        "example/rules",
        "2.0.0",
        "rev2",
        "pub fn value() -> i32 { 2 }",
    );
    let hosts = [("old".into(), old), ("new".into(), new)].into();
    let graph = resolve(&sources, &hosts, None, ResolutionOptions::default());
    let cancel = CancellationToken::default();
    for root in graph.members().values() {
        assert_eq!(
            graph
                .capture(&sources, root, ModuleLimits::default(), &cancel)
                .unwrap()
                .packages()
                .len(),
            2
        );
    }
    let combined = input(&[(
        "Kagari.toml",
        "[package]\nname = 'app'\nversion = '1.0.0'\nnamespace = 'example/app'\n[dependencies]\none = { package = 'rules', host = 'old' }\ntwo = { package = 'rules', host = 'new' }",
    )]);
    let error = ResolvedGraph::resolve(
        &combined,
        &workspace(&combined),
        &hosts,
        None,
        ResolutionOptions::default(),
        GraphLimits::default(),
        &cancel,
    )
    .unwrap_err();
    assert!(
        error.to_string().contains("conflicting selections"),
        "{error}"
    );
    assert!(
        error.to_string().contains("one (") && error.to_string().contains("two ("),
        "{error}"
    );
}

#[test]
fn dependency_alias_only_changes_invalidate_program_inputs() {
    let old = host(
        "example/rules",
        "1.0.0",
        "rev1",
        "pub fn value() -> i32 { 1 }",
    );
    let hosts = [("rules".into(), old)].into();
    let cancel = CancellationToken::default();
    let mut fingerprints = Vec::new();
    for alias in ["one", "two"] {
        let sources = input(&[
            (
                "Kagari.toml",
                &format!(
                    "[package]\nname = 'app'\nversion = '1.0.0'\nnamespace = 'example/app'\n[dependencies]\n{alias} = {{ package = 'rules', host = 'rules' }}"
                ),
            ),
            ("src/lib.kgr", "pub fn run() {}"),
        ]);
        let graph = resolve(&sources, &hosts, None, ResolutionOptions::default());
        fingerprints.push(
            graph
                .capture(
                    &sources,
                    &graph.members()[""],
                    ModuleLimits::default(),
                    &cancel,
                )
                .unwrap()
                .fingerprint()
                .to_owned(),
        );
    }
    assert_ne!(fingerprints[0], fingerprints[1]);
}

#[test]
fn lock_publication_is_atomic_policy_checked_and_cancellation_preserves_the_previous_file() {
    let root = scratch();
    let disk = FilesystemSources::new(&root).unwrap();
    let sources = local_sources("");
    let graph = resolve(
        &sources,
        &BTreeMap::new(),
        None,
        ResolutionOptions::default(),
    );
    let lock = Lockfile::from_graph(&graph);
    let cancel = CancellationToken::default();
    assert!(
        lock.publish(
            &disk,
            "",
            ResolutionOptions {
                locked: true,
                offline: false
            },
            &cancel
        )
        .is_err()
    );
    assert!(!root.join("Kagari.lock").exists());
    assert!(
        lock.publish(
            &disk,
            "",
            ResolutionOptions {
                locked: false,
                offline: true
            },
            &cancel
        )
        .unwrap()
    );
    let before = fs::read(root.join("Kagari.lock")).unwrap();
    assert!(
        !lock
            .publish(
                &disk,
                "",
                ResolutionOptions {
                    locked: true,
                    offline: true
                },
                &cancel
            )
            .unwrap()
    );
    let standalone = host_user("1");
    let different = Lockfile::from_graph(&resolve(
        &standalone,
        &[("rules".into(), host("example/rules", "1.0.0", "rev1", ""))].into(),
        None,
        ResolutionOptions::default(),
    ));
    assert!(
        different
            .publish(
                &disk,
                "",
                ResolutionOptions {
                    locked: true,
                    offline: false
                },
                &cancel
            )
            .is_err()
    );
    assert_eq!(fs::read(root.join("Kagari.lock")).unwrap(), before);
    let cancelled = CancellationToken::default();
    cancelled.cancel();
    assert!(
        different
            .publish(&disk, "", ResolutionOptions::default(), &cancelled)
            .is_err()
    );
    assert_eq!(fs::read(root.join("Kagari.lock")).unwrap(), before);
    assert!(
        different
            .publish(&disk, "", ResolutionOptions::default(), &cancel)
            .unwrap()
    );
    assert_eq!(
        Lockfile::load(&disk, "", &cancel).unwrap().unwrap(),
        different
    );
    assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
    let outside = scratch();
    fs::write(outside.join("Kagari.lock"), "outside sentinel").unwrap();
    link_directory(&outside, &root.join("outside")).unwrap();
    assert!(
        lock.publish(&disk, "outside", ResolutionOptions::default(), &cancel)
            .is_err()
    );
    assert_eq!(
        fs::read_to_string(outside.join("Kagari.lock")).unwrap(),
        "outside sentinel"
    );
}

#[test]
fn lock_decoder_rejects_duplicate_dangling_cyclic_and_inconsistent_graph_records() {
    let sources = local_sources("");
    let graph = resolve(
        &sources,
        &BTreeMap::new(),
        None,
        ResolutionOptions::default(),
    );
    let encoded = Lockfile::from_graph(&graph).encode().unwrap();
    let base: toml::Value = toml::from_str(&encoded).unwrap();
    let mut duplicate = base.clone();
    let packages = duplicate["packages"].as_array_mut().unwrap();
    packages.push(packages[0].clone());
    let mut dangling = base.clone();
    dangling["members"]["game"]["version"] = "99.0.0".into();
    let game_index = base["packages"]
        .as_array()
        .unwrap()
        .iter()
        .position(|package| package["selection"]["family"]["name"].as_str() == Some("game"))
        .unwrap();
    let mut cyclic = base.clone();
    cyclic["packages"][game_index]["dependencies"]["shared"]["package"] =
        cyclic["packages"][game_index]["selection"].clone();
    let mut inconsistent = base.clone();
    inconsistent["packages"][game_index]["dependencies"]["shared"]
        .as_table_mut()
        .unwrap()
        .insert("host_key".into(), "unapproved".into());
    let mut unknown = base.clone();
    unknown["packages"][game_index]["selection"]["family"]
        .as_table_mut()
        .unwrap()
        .insert("extra".into(), true.into());
    let cancel = CancellationToken::default();
    for (value, expected) in [
        (duplicate, "duplicate"),
        (dangling, "dangling"),
        (cyclic, "cycle"),
        (inconsistent, "disagrees"),
        (unknown, "unknown field"),
    ] {
        let error =
            Lockfile::parse("Kagari.lock", &toml::to_string(&value).unwrap(), &cancel).unwrap_err();
        assert!(error.to_string().contains(expected), "{error}");
    }
    cancel.cancel();
    assert!(
        Lockfile::parse("Kagari.lock", &encoded, &cancel)
            .unwrap_err()
            .to_string()
            .contains("cancelled")
    );
}

#[test]
fn diamond_dependencies_are_captured_once_and_same_named_other_sources_stay_distinct() {
    let sources = input(&[
        (
            "Kagari.toml",
            "[package]\nname = 'app'\nversion = '1.0.0'\n[workspace]\nnamespace = 'example/diamond'\n[dependencies]\nleft = { path = 'left' }\nright = { path = 'right' }",
        ),
        ("src/lib.kgr", "pub fn run() {}"),
        (
            "left/Kagari.toml",
            "[package]\nname = 'left'\nversion = '1.0.0'\n[dependencies]\nrules = { host = 'shared' }",
        ),
        ("left/src/lib.kgr", ""),
        (
            "right/Kagari.toml",
            "[package]\nname = 'right'\nversion = '1.0.0'\n[dependencies]\nrules = { host = 'shared' }\nother = { package = 'rules', host = 'independent' }",
        ),
        ("right/src/lib.kgr", ""),
    ]);
    let shared = host("example/shared", "1.0.0", "rev1", "");
    let independent = host("other/shared", "1.0.0", "rev1", "");
    let catalog = [
        ("shared".into(), shared.clone()),
        ("independent".into(), independent.clone()),
    ]
    .into();
    let graph = resolve(&sources, &catalog, None, ResolutionOptions::default());
    assert_eq!(graph.packages().len(), 5);
    let captured = graph
        .capture(
            &sources,
            &graph.members()[""],
            ModuleLimits::default(),
            &CancellationToken::default(),
        )
        .unwrap();
    assert_eq!(captured.packages().len(), 5);
    assert!(captured.packages().contains_key(shared.selection()));
    assert!(captured.packages().contains_key(independent.selection()));
}

#[test]
fn relocated_acquisition_roots_keep_identical_locks_and_program_fingerprints() {
    let cancel = CancellationToken::default();
    let mut results = Vec::new();
    for directory in ["checkout_a", "elsewhere/checkout_b"] {
        let sources = input(&[
            (
                &format!("{directory}/Kagari.toml"),
                "[package]\nname = 'app'\nnamespace = 'example/portable'\nversion = '1.0.0'",
            ),
            (
                &format!("{directory}/src/lib.kgr"),
                "mod child; pub fn run() {}",
            ),
            (&format!("{directory}/src/child.kgr"), "pub fn helper() {}"),
        ]);
        let workspace = WorkspaceInputs::load(
            &sources,
            directory,
            None,
            WorkspaceLimits::default(),
            &cancel,
        )
        .unwrap();
        let graph = ResolvedGraph::resolve(
            &sources,
            &workspace,
            &BTreeMap::new(),
            None,
            ResolutionOptions::default(),
            GraphLimits::default(),
            &cancel,
        )
        .unwrap();
        let captured = graph
            .capture(
                &sources,
                &graph.members()[""],
                ModuleLimits::default(),
                &cancel,
            )
            .unwrap();
        results.push((
            Lockfile::from_graph(&graph),
            captured.fingerprint().to_owned(),
        ));
    }
    assert_eq!(results[0], results[1]);
}

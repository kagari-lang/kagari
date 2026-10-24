use crate::test_support as support;

use crate::{
    error::PackageError,
    filesystem::FilesystemSources,
    identity::{PackageFamily, source_namespace},
    modules::{ModuleLimits, ModuleTree, VirtualSources},
};
use kagari_common::cancellation::CancellationToken;
use rstest::rstest;
use std::{fs, sync::Arc};
use support::{link_directory, scratch};

#[test]
fn declared_module_tree_has_unnamed_root_and_one_owner_for_inline_and_external_sources() {
    let input = support::sources(&[
        (
            "src/lib.kgr",
            "pub mod api { mod details; } mod storage; fn main() {}",
        ),
        ("src/api/details.kgr", "mod leaf;"),
        ("src/api/details/leaf/mod.kgr", "fn helper() {}"),
        ("src/storage/mod.kgr", "mod child;"),
        ("src/storage/child.kgr", "fn helper() {}"),
        ("src/unused.kgr", "not valid syntax"),
    ]);
    let tree = ModuleTree::load(
        &input,
        "src/lib.kgr",
        ModuleLimits::default(),
        &CancellationToken::default(),
    )
    .unwrap();
    assert_eq!(tree.modules.len(), 6);
    assert_eq!(tree.files.len(), 5);
    assert_eq!(tree.modules[&Vec::<String>::new()].file, "src/lib.kgr");
    let api = &tree.modules[&vec!["api".into()]];
    assert_eq!(api.file, "src/lib.kgr");
    let range = api.inline_range.clone().unwrap();
    assert_eq!(
        &tree.files["src/lib.kgr"].text[range],
        "pub mod api { mod details; }"
    );
    assert_eq!(
        tree.modules[&vec!["api".into(), "details".into(), "leaf".into()]].file,
        "src/api/details/leaf/mod.kgr"
    );
}

#[rstest]
#[case::ambiguous(
    &[("lib.kgr", "mod child;"), ("child.kgr", ""), ("child/mod.kgr", "")],
    "ambiguous module child"
)]
#[case::duplicate(&[("lib.kgr", "mod child {} mod child {}")], "duplicate module child")]
#[case::missing(&[("lib.kgr", "mod child;")], "missing module child")]
#[case::invalid_syntax(&[("lib.kgr", "fn broken(")], "lib.kgr")]
fn module_discovery_rejections(#[case] files: &[(&str, &str)], #[case] expected: &str) {
    let cancel = CancellationToken::default();
    let error = ModuleTree::load(
        &support::sources(files),
        "lib.kgr",
        ModuleLimits::default(),
        &cancel,
    )
    .unwrap_err();
    assert!(error.to_string().contains(expected), "{error}");
}

#[test]
fn input_paths_limits_and_cancellation_are_enforced_before_publication() {
    let cancel = CancellationToken::default();
    for name in [
        "../outside.kgr",
        "/absolute.kgr",
        "C:/absolute.kgr",
        "folder\\file.kgr",
    ] {
        assert!(VirtualSources::new([(name.into(), Arc::from(""))], &cancel).is_err());
    }
    assert!(
        VirtualSources::new(
            [
                ("a/../lib.kgr".into(), Arc::from("")),
                ("lib.kgr".into(), Arc::from("")),
            ],
            &cancel
        )
        .is_err()
    );
    let input = support::sources(&[("lib.kgr", "mod child {}")]);
    for limits in [
        ModuleLimits {
            max_modules: 1,
            ..ModuleLimits::default()
        },
        ModuleLimits {
            max_file_bytes: 1,
            ..ModuleLimits::default()
        },
        ModuleLimits {
            max_total_bytes: 1,
            ..ModuleLimits::default()
        },
    ] {
        assert!(
            ModuleTree::load(&input, "lib.kgr", limits, &cancel)
                .unwrap_err()
                .to_string()
                .contains("limit")
        );
    }
    cancel.cancel();
    assert!(matches!(
        ModuleTree::load(&input, "lib.kgr", ModuleLimits::default(), &cancel),
        Err(PackageError::Cancelled)
    ));
}

#[test]
fn filesystem_and_virtual_inputs_preserve_portable_module_identity_after_relocation() {
    let cancel = CancellationToken::default();
    let files = [
        ("src/lib.kgr", "mod data;"),
        ("src/data.kgr", "pub fn count() -> i32 { 1 }"),
    ];
    let virtual_tree = ModuleTree::load(
        &support::sources(&files),
        "src/lib.kgr",
        ModuleLimits::default(),
        &cancel,
    )
    .unwrap();
    let family = PackageFamily::new("example/game", "packages/game", "game").unwrap();
    for _ in 0..2 {
        let root = scratch();
        fs::create_dir_all(root.join("src")).unwrap();
        for (name, text) in files {
            fs::write(root.join(name), text).unwrap();
        }
        let provider = FilesystemSources::new(&root).unwrap();
        let disk =
            ModuleTree::load(&provider, "src/lib.kgr", ModuleLimits::default(), &cancel).unwrap();
        assert_eq!(virtual_tree.modules, disk.modules);
        for (name, source) in &virtual_tree.files {
            assert_eq!(source.text, disk.files[name].text);
        }
        assert_eq!(
            family.package_id(),
            PackageFamily::new("example/game", "packages/./game", "game")
                .unwrap()
                .package_id()
        );
        assert!(provider.canonical_path("../outside").is_err());
        assert!(
            ModuleTree::load(
                &provider,
                "src/lib.kgr",
                ModuleLimits {
                    max_file_bytes: 1,
                    ..ModuleLimits::default()
                },
                &cancel
            )
            .unwrap_err()
            .to_string()
            .contains("limit")
        );
    }
    assert_ne!(
        family.package_id(),
        PackageFamily::new("other/game", "packages/game", "game")
            .unwrap()
            .package_id()
    );
    assert_ne!(
        family.package_id(),
        PackageFamily::new("example/game", "other/game", "game")
            .unwrap()
            .package_id()
    );
    assert_eq!(
        source_namespace(Some("example/game"), Some("example/game")).unwrap(),
        "example/game"
    );
    assert!(source_namespace(Some("example/game"), Some("other/game")).is_err());
    assert!(source_namespace(None, None).is_err());
}

#[test]
fn filesystem_rejects_escaped_links_and_duplicate_physical_module_sources() {
    let root = scratch();
    let outside = scratch();
    fs::create_dir(root.join("shared")).unwrap();
    fs::write(root.join("shared/mod.kgr"), "").unwrap();
    fs::write(outside.join("mod.kgr"), "").unwrap();
    link_directory(&outside, &root.join("escape")).unwrap();
    let provider = FilesystemSources::new(&root).unwrap();
    assert!(
        provider
            .canonical_path("escape/mod.kgr")
            .unwrap_err()
            .to_string()
            .contains("escapes")
    );
    link_directory(&root.join("shared"), &root.join("one")).unwrap();
    link_directory(&root.join("shared"), &root.join("two")).unwrap();
    fs::write(root.join("lib.kgr"), "mod one; mod two;").unwrap();
    let error = ModuleTree::load(
        &provider,
        "lib.kgr",
        ModuleLimits::default(),
        &CancellationToken::default(),
    )
    .unwrap_err();
    assert!(
        error.to_string().contains("source already assigned"),
        "{error}"
    );
}

#[test]
fn recovery_retains_invalid_text_and_independent_siblings_without_complete_evidence() {
    use crate::modules::ModuleDiscovery;

    let input = support::sources(&[
        ("lib.kgr", "mod broken; mod absent; mod healthy;"),
        ("broken.kgr", "mod hidden; fn broken("),
        ("broken/hidden.kgr", "fn hidden() {}"),
        ("healthy.kgr", "mod leaf;"),
        ("healthy/leaf.kgr", "fn leaf() {}"),
    ]);
    let discovery = ModuleDiscovery::load(
        &input,
        "lib.kgr",
        ModuleLimits::default(),
        &CancellationToken::default(),
    )
    .unwrap();
    assert!(!discovery.is_complete());
    assert_eq!(
        discovery.files()["broken.kgr"].text.as_ref(),
        "mod hidden; fn broken("
    );
    assert!(discovery.files().contains_key("healthy/leaf.kgr"));
    assert!(!discovery.files().contains_key("broken/hidden.kgr"));
    assert!(!discovery.diagnostics()[0].1.is_empty());
    assert!(
        discovery
            .errors()
            .iter()
            .any(|error| error.to_string().contains("missing module absent"))
    );
    assert!(discovery.into_complete().is_err());
}

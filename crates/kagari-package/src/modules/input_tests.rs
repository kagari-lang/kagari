use crate::test_support as support;

use crate::{
    error::PackageError,
    filesystem::FilesystemSources,
    identity::{PackageFamily, source_namespace},
    manifest::{DependencySource, Inheritance, Manifest},
    modules::{ModuleLimits, ModuleTree, VirtualSources},
};
use kagari_common::cancellation::CancellationToken;
use std::{fs, sync::Arc};
use support::{link_directory, scratch};

fn manifest(text: &str) -> Manifest {
    Manifest::parse("Kagari.toml", text, &CancellationToken::default()).unwrap()
}

fn sources(files: &[(&str, &str)]) -> VirtualSources {
    VirtualSources::new(
        files
            .iter()
            .map(|(name, text)| (name.to_string(), Arc::from(*text))),
        &CancellationToken::default(),
    )
    .unwrap()
}

#[test]
fn inheritance_preserves_declaration_bases_and_explicit_dependency_access() {
    let workspace = manifest(
        r#"
[workspace]
members = ["packages/*"]
[workspace.dependencies]
shared = { package = "shared-rules", path = "shared", version = "^1.2" }
unused = { host = "private-provider" }
"#,
    );
    let member = manifest(
        r#"
[package]
name = "game"
version = "1.0.0"
[dependencies]
shared.workspace = true
local = { path = "../local" }
service = { package = "service-api", host = "approved/service", version = "2" }
"#,
    );
    let normalized = member
        .normalize(
            "packages/game/Kagari.toml",
            "packages/game",
            Some(Inheritance {
                directory: "",
                manifest_origin: "Kagari.toml",
                workspace: workspace.workspace.as_ref().unwrap(),
            }),
            &CancellationToken::default(),
        )
        .unwrap()
        .unwrap();
    assert_eq!(normalized.root, "packages/game/src/lib.kgr");
    assert_eq!(normalized.dependencies.len(), 3);
    let shared = &normalized.dependencies["shared"];
    assert_eq!(shared.package, "shared-rules");
    assert_eq!(shared.source, DependencySource::Path("shared".into()));
    assert!(
        shared
            .version
            .as_ref()
            .unwrap()
            .matches(&"1.3.0".parse().unwrap())
    );
    assert!(
        !shared
            .version
            .as_ref()
            .unwrap()
            .matches(&"2.0.0".parse().unwrap())
    );
    assert_eq!(shared.origin, "Kagari.toml: dependency shared");
    assert_eq!(
        normalized.dependencies["local"].source,
        DependencySource::Path("packages/local".into())
    );
    assert_eq!(
        normalized.dependencies["service"].source,
        DependencySource::Host("approved/service".into())
    );
    assert!(!normalized.dependencies.contains_key("unused"));
}

#[test]
fn invalid_manifests_do_not_silently_accept_unsupported_or_conflicting_settings() {
    let prefix = "[package]\nname = 'app'\nversion = '1.0.0'\n";
    let cancel = CancellationToken::default();
    for (body, expected) in [
        ("build = 'script.kgr'", "unknown field"),
        (
            "[dependencies]\na = { version = '1' }",
            "remote acquisition is unsupported",
        ),
        (
            "[dependencies]\na = { path = '.', host = 'a' }",
            "exactly one",
        ),
        (
            "[dependencies]\na = { workspace = true, path = '.' }",
            "inheritance must be exactly",
        ),
        (
            "[dependencies]\na = { workspace = false }",
            "inheritance must be exactly",
        ),
        (
            "[dependencies]\nfn = { path = '.' }",
            "invalid dependency alias",
        ),
        (
            "[workspace.dependencies]\na = { workspace = true }",
            "inheritance must be exactly",
        ),
    ] {
        let error = Manifest::parse("input", &format!("{prefix}{body}"), &cancel).unwrap_err();
        assert!(error.to_string().contains(expected), "{error}");
    }
    assert!(
        Manifest::parse(
            "input",
            "[workspace]\n[dependencies]\na = { path = '.' }",
            &cancel
        )
        .unwrap_err()
        .to_string()
        .contains("virtual workspaces")
    );
    let inherited = manifest(&format!("{prefix}[dependencies]\na.workspace = true"));
    assert!(
        inherited
            .normalize("input", "app", None, &cancel)
            .unwrap_err()
            .to_string()
            .contains("owning workspace")
    );
    let no_deps = manifest("[workspace]");
    assert!(
        inherited
            .normalize(
                "input",
                "app",
                Some(Inheritance {
                    directory: "",
                    manifest_origin: "root",
                    workspace: no_deps.workspace.as_ref().unwrap(),
                }),
                &cancel
            )
            .unwrap_err()
            .to_string()
            .contains("missing workspace dependency")
    );
}

#[test]
fn declared_module_tree_has_unnamed_root_and_one_owner_for_inline_and_external_sources() {
    let input = sources(&[
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

#[test]
fn module_discovery_rejects_ambiguity_duplicates_missing_files_and_invalid_syntax() {
    let cancel = CancellationToken::default();
    for (files, expected) in [
        (
            vec![
                ("lib.kgr", "mod child;"),
                ("child.kgr", ""),
                ("child/mod.kgr", ""),
            ],
            "ambiguous module child",
        ),
        (
            vec![("lib.kgr", "mod child {} mod child {}")],
            "duplicate module child",
        ),
        (vec![("lib.kgr", "mod child;")], "missing module child"),
        (vec![("lib.kgr", "fn broken(")], "lib.kgr"),
    ] {
        let error = ModuleTree::load(
            &sources(&files),
            "lib.kgr",
            ModuleLimits::default(),
            &cancel,
        )
        .unwrap_err();
        assert!(error.to_string().contains(expected), "{error}");
    }
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
    let input = sources(&[("lib.kgr", "mod child {}")]);
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
    assert!(matches!(
        Manifest::parse("input", "[workspace]", &cancel),
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
        &sources(&files),
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

mod support;

use kagari_common::cancellation::CancellationToken;
use kagari_package::{
    filesystem::FilesystemSources,
    manifest::DependencySource,
    modules::VirtualSources,
    workspace::{MemberSelection, WorkspaceInputs, WorkspaceLimits},
};
use std::{fs, sync::Arc};
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

fn load(files: &[(&str, &str)], directory: &str) -> WorkspaceInputs {
    WorkspaceInputs::load(
        &input(files),
        directory,
        None,
        WorkspaceLimits::default(),
        &CancellationToken::default(),
    )
    .unwrap()
}

fn selected(workspace: &WorkspaceInputs, selection: MemberSelection) -> Vec<String> {
    workspace
        .select(&selection)
        .unwrap()
        .iter()
        .map(|(_, package)| package.manifest.name.clone())
        .collect()
}

#[test]
fn virtual_workspace_discovers_globs_auto_members_inheritance_and_member_defaults() {
    let files = [
        (
            "Kagari.toml",
            r#"
[workspace]
namespace = "example/project"
members = ["packages/*"]
exclude = ["packages/excluded"]
default-members = ["packages/game"]
[workspace.dependencies]
shared = { path = "shared" }
"#,
        ),
        (
            "packages/game/Kagari.toml",
            "[package]\nname = 'game'\nversion = '1.0.0'\n[dependencies]\nshared.workspace = true",
        ),
        (
            "packages/tool/Kagari.toml",
            "[package]\nname = 'tool'\nversion = '1.0.0'",
        ),
        (
            "packages/excluded/Kagari.toml",
            "malformed excluded manifest",
        ),
        (
            "shared/Kagari.toml",
            "[package]\nname = 'shared'\nversion = '1.0.0'\n[dependencies]\nleaf = { path = '../leaf' }",
        ),
        (
            "leaf/Kagari.toml",
            "[package]\nname = 'leaf'\nversion = '1.0.0'",
        ),
        ("packages/tool/src/lib.kgr", "invalid unselected body"),
    ];
    let workspace = load(&files, "");
    assert_eq!(workspace.members.len(), 4);
    assert_eq!(selected(&workspace, MemberSelection::Default), ["game"]);
    assert_eq!(
        selected(&workspace, MemberSelection::Workspace),
        ["leaf", "game", "tool", "shared"]
    );
    assert_eq!(
        selected(
            &workspace,
            MemberSelection::Names(vec!["shared".into(), "tool".into()])
        ),
        ["tool", "shared"]
    );
    assert_eq!(
        workspace.members["packages/game"].manifest.dependencies["shared"].source,
        DependencySource::Path("shared".into())
    );
    assert!(
        workspace.members["packages/tool"]
            .manifest
            .dependencies
            .is_empty()
    );
    let member = load(&files, "packages/tool");
    assert_eq!(selected(&member, MemberSelection::Default), ["tool"]);
    assert_eq!(member.default_members, workspace.default_members);
    assert_eq!(
        member.members["shared"].family.package_id(),
        workspace.members["shared"].family.package_id()
    );
    assert!(
        workspace
            .select(&MemberSelection::Names(vec!["missing".into()]))
            .is_err()
    );
}

#[test]
fn root_package_and_standalone_select_one_root_without_creating_dependencies() {
    let root = load(
        &[
            (
                "Kagari.toml",
                "[package]\nname = 'root'\nversion = '1.0.0'\n[workspace]\nnamespace = 'example/root'\nmembers = ['child']",
            ),
            (
                "child/Kagari.toml",
                "[package]\nname = 'child'\nversion = '1.0.0'",
            ),
        ],
        "",
    );
    assert_eq!(selected(&root, MemberSelection::Default), ["root"]);
    assert!(root.members[""].manifest.dependencies.is_empty());
    let standalone = load(
        &[(
            "app/Kagari.toml",
            "[package]\nname = 'app'\nversion = '1.0.0'\nnamespace = 'example/app'",
        )],
        "app",
    );
    assert_eq!(selected(&standalone, MemberSelection::Workspace), ["app"]);
    assert_eq!(standalone.members["app"].family.directory(), "");
}

#[test]
fn explicit_external_members_keep_their_workspace_and_portable_relative_locator() {
    let files = [
        (
            "project/Kagari.toml",
            "[workspace]\nnamespace = 'example/project'\nmembers = ['../external']",
        ),
        (
            "external/Kagari.toml",
            "[package]\nname = 'external'\nversion = '1.0.0'\nworkspace = '../project'",
        ),
    ];
    let workspace = load(&files, "external");
    assert_eq!(workspace.directory, "project");
    assert_eq!(
        workspace.members["external"].family.directory(),
        "../external"
    );
    assert_eq!(selected(&workspace, MemberSelection::Default), ["external"]);
}

#[test]
fn excluded_packages_can_be_standalone_but_unlisted_packages_cannot_inherit_membership() {
    let files = [
        (
            "Kagari.toml",
            "[workspace]\nnamespace = 'example/root'\nexclude = ['excluded']",
        ),
        (
            "excluded/Kagari.toml",
            "[package]\nname = 'excluded'\nversion = '1.0.0'\nnamespace = 'example/excluded'",
        ),
        (
            "unlisted/Kagari.toml",
            "[package]\nname = 'unlisted'\nversion = '1.0.0'",
        ),
    ];
    assert_eq!(load(&files, "excluded").namespace, "example/excluded");
    let error = WorkspaceInputs::load(
        &input(&files),
        "unlisted",
        None,
        WorkspaceLimits::default(),
        &CancellationToken::default(),
    )
    .unwrap_err();
    assert!(error.to_string().contains("not a member"), "{error}");
}

#[test]
fn nested_workspaces_are_not_merged_and_duplicate_member_names_are_rejected() {
    let cancel = CancellationToken::default();
    for (files, expected) in [
        (
            vec![
                (
                    "Kagari.toml",
                    "[workspace]\nnamespace = 'outer'\nmembers = ['nested/child']",
                ),
                (
                    "nested/Kagari.toml",
                    "[workspace]\nnamespace = 'nested'\nmembers = ['child']",
                ),
                (
                    "nested/child/Kagari.toml",
                    "[package]\nname = 'child'\nversion = '1.0.0'",
                ),
            ],
            "another workspace",
        ),
        (
            vec![
                (
                    "Kagari.toml",
                    "[workspace]\nnamespace = 'outer'\nmembers = ['a', 'b']",
                ),
                (
                    "a/Kagari.toml",
                    "[package]\nname = 'same'\nversion = '1.0.0'",
                ),
                (
                    "b/Kagari.toml",
                    "[package]\nname = 'same'\nversion = '2.0.0'",
                ),
            ],
            "duplicate workspace package name",
        ),
        (
            vec![(
                "Kagari.toml",
                "[workspace]\nnamespace = 'outer'\ndefault-members = ['absent']",
            )],
            "not a workspace member",
        ),
    ] {
        let error = WorkspaceInputs::load(
            &input(&files),
            "",
            None,
            WorkspaceLimits::default(),
            &cancel,
        )
        .unwrap_err();
        assert!(error.to_string().contains(expected), "{error}");
    }
}

#[test]
fn whole_workspace_configuration_errors_block_single_member_selection() {
    let files = [
        (
            "Kagari.toml",
            "[workspace]\nnamespace = 'example/root'\nmembers = ['good', 'bad']",
        ),
        (
            "good/Kagari.toml",
            "[package]\nname = 'good'\nversion = '1.0.0'",
        ),
        (
            "bad/Kagari.toml",
            "[package]\nname = 'bad'\nversion = '1.0.0'\n[dependencies]\nmissing.workspace = true",
        ),
    ];
    let error = WorkspaceInputs::load(
        &input(&files),
        "good",
        None,
        WorkspaceLimits::default(),
        &CancellationToken::default(),
    )
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("missing workspace dependency missing"),
        "{error}"
    );
}

#[test]
fn discovery_checks_namespace_input_bounds_and_cancellation() {
    let sources = input(&[
        (
            "Kagari.toml",
            "[workspace]\nnamespace = 'example/root'\nmembers = ['packages/*']",
        ),
        (
            "packages/a/Kagari.toml",
            "[package]\nname = 'a'\nversion = '1.0.0'",
        ),
    ]);
    let cancel = CancellationToken::default();
    for limits in [
        WorkspaceLimits {
            max_manifests: 1,
            ..WorkspaceLimits::default()
        },
        WorkspaceLimits {
            max_directories: 1,
            ..WorkspaceLimits::default()
        },
    ] {
        assert!(
            WorkspaceInputs::load(&sources, "", None, limits, &cancel)
                .unwrap_err()
                .to_string()
                .contains("limit")
        );
    }
    assert!(
        WorkspaceInputs::load(
            &sources,
            "",
            Some("different/root"),
            WorkspaceLimits::default(),
            &cancel
        )
        .unwrap_err()
        .to_string()
        .contains("disagree")
    );
    cancel.cancel();
    assert!(
        WorkspaceInputs::load(&sources, "", None, WorkspaceLimits::default(), &cancel)
            .unwrap_err()
            .to_string()
            .contains("cancelled")
    );
}

#[test]
fn filesystem_workspace_matches_virtual_members_and_defaults_after_relocation() {
    let files = [
        (
            "project/Kagari.toml",
            "[workspace]\nnamespace = 'example/project'\nmembers = ['packages/*', '../external']\ndefault-members = ['packages/a']",
        ),
        (
            "project/packages/a/Kagari.toml",
            "[package]\nname = 'a'\nversion = '1.0.0'",
        ),
        (
            "external/Kagari.toml",
            "[package]\nname = 'external'\nversion = '1.0.0'\nworkspace = '../project'",
        ),
    ];
    let virtual_workspace = load(&files, "project");
    for _ in 0..2 {
        let root = scratch();
        for (name, text) in files {
            let path = root.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        let workspace = WorkspaceInputs::load(
            &FilesystemSources::new(&root).unwrap(),
            "project",
            None,
            WorkspaceLimits::default(),
            &CancellationToken::default(),
        )
        .unwrap();
        assert_eq!(workspace.default_members, virtual_workspace.default_members);
        let families = |workspace: &WorkspaceInputs| {
            workspace
                .members
                .iter()
                .map(|(directory, package)| (directory.clone(), package.family.package_id()))
                .collect::<Vec<_>>()
        };
        assert_eq!(families(&workspace), families(&virtual_workspace));
    }
}

#[test]
fn recursive_workspace_patterns_fail_at_depth_bounds_instead_of_publishing_partial_membership() {
    let files = [
        (
            "Kagari.toml",
            "[workspace]\nnamespace = 'example/root'\nmembers = ['packages/**/member']",
        ),
        (
            "packages/one/two/member/Kagari.toml",
            "[package]\nname = 'member'\nversion = '1.0.0'",
        ),
    ];
    let provider = input(&files);
    let cancel = CancellationToken::default();
    let error = WorkspaceInputs::load(
        &provider,
        "",
        None,
        WorkspaceLimits {
            max_depth: 1,
            ..WorkspaceLimits::default()
        },
        &cancel,
    )
    .unwrap_err();
    assert!(error.to_string().contains("depth exceeds limit"), "{error}");
    assert_eq!(load(&files, "").members.len(), 1);
}

#[test]
fn workspace_rejects_two_members_pointing_at_one_physical_manifest() {
    let root = scratch();
    fs::create_dir(root.join("shared")).unwrap();
    fs::write(
        root.join("Kagari.toml"),
        "[workspace]\nnamespace = 'example/root'\nmembers = ['one', 'two']",
    )
    .unwrap();
    fs::write(
        root.join("shared/Kagari.toml"),
        "[package]\nname = 'shared'\nversion = '1.0.0'",
    )
    .unwrap();
    link_directory(&root.join("shared"), &root.join("one")).unwrap();
    link_directory(&root.join("shared"), &root.join("two")).unwrap();
    let error = WorkspaceInputs::load(
        &FilesystemSources::new(&root).unwrap(),
        "",
        None,
        WorkspaceLimits::default(),
        &CancellationToken::default(),
    )
    .unwrap_err();
    assert!(
        error.to_string().contains("manifest already loaded"),
        "{error}"
    );
}

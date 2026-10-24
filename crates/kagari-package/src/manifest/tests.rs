use crate::{
    error::PackageError,
    manifest::{DependencySource, Inheritance, Manifest},
};
use kagari_common::cancellation::CancellationToken;
use rstest::rstest;

const PACKAGE_PREFIX: &str = "[package]\nname = 'app'\nversion = '1.0.0'\n";

fn manifest(text: &str) -> Manifest {
    Manifest::parse("Kagari.toml", text, &CancellationToken::default()).unwrap()
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

#[rstest]
#[case::unsupported_build("build = 'script.kgr'", "unknown field")]
#[case::remote_version(
    "[dependencies]\na = { version = '1' }",
    "remote acquisition is unsupported"
)]
#[case::path_and_host("[dependencies]\na = { path = '.', host = 'a' }", "exactly one")]
#[case::inheritance_and_path(
    "[dependencies]\na = { workspace = true, path = '.' }",
    "inheritance must be exactly"
)]
#[case::false_inheritance(
    "[dependencies]\na = { workspace = false }",
    "inheritance must be exactly"
)]
#[case::reserved_alias("[dependencies]\nfn = { path = '.' }", "invalid dependency alias")]
#[case::workspace_inheritance(
    "[workspace.dependencies]\na = { workspace = true }",
    "inheritance must be exactly"
)]
fn invalid_manifest_settings(#[case] body: &str, #[case] expected: &str) {
    let prefix = PACKAGE_PREFIX;
    let cancel = CancellationToken::default();
    let error = Manifest::parse("input", &format!("{prefix}{body}"), &cancel).unwrap_err();
    assert!(error.to_string().contains(expected), "{error}");
}

#[test]
fn virtual_workspaces_cannot_declare_package_dependencies() {
    let cancel = CancellationToken::default();
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
}

#[test]
fn inherited_dependencies_require_an_owning_workspace() {
    let prefix = PACKAGE_PREFIX;
    let cancel = CancellationToken::default();
    let inherited = manifest(&format!("{prefix}[dependencies]\na.workspace = true"));
    assert!(
        inherited
            .normalize("input", "app", None, &cancel)
            .unwrap_err()
            .to_string()
            .contains("owning workspace")
    );
}

#[test]
fn inherited_dependencies_require_a_workspace_declaration() {
    let prefix = PACKAGE_PREFIX;
    let cancel = CancellationToken::default();
    let inherited = manifest(&format!("{prefix}[dependencies]\na.workspace = true"));
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
fn cancellation_precedes_manifest_parsing() {
    let cancel = CancellationToken::default();
    cancel.cancel();
    assert!(matches!(
        Manifest::parse("input", "[workspace]", &cancel),
        Err(PackageError::Cancelled)
    ));
}

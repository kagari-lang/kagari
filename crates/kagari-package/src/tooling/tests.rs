use super::*;
use crate::{
    input::InputTree,
    modules::{ModuleLimits, SourceInput, VirtualSources},
    project::ProjectOptions,
    workspace::MemberSelection,
};
use std::sync::Arc;

fn sources(repaired: bool, changed: bool) -> VirtualSources {
    let mut files = vec![
        (
            "Kagari.toml",
            include_str!("../../tests/fixtures/tooling/Kagari.toml"),
        ),
        (
            "app/Kagari.toml",
            include_str!("../../tests/fixtures/tooling/app.toml"),
        ),
        (
            "api/Kagari.toml",
            include_str!("../../tests/fixtures/tooling/api.toml"),
        ),
        (
            "app/src/lib.kgr",
            include_str!("../../tests/fixtures/tooling/app.kgr"),
        ),
        (
            "app/src/healthy.kgr",
            include_str!("../../tests/fixtures/tooling/healthy.kgr"),
        ),
        (
            "app/src/broken.kgr",
            if repaired {
                include_str!("../../tests/fixtures/tooling/repaired.kgr")
            } else {
                include_str!("../../tests/fixtures/tooling/broken.kgr")
            },
        ),
        (
            "api/src/lib.kgr",
            if changed {
                include_str!("../../tests/fixtures/tooling/api_changed.kgr")
            } else {
                include_str!("../../tests/fixtures/tooling/api.kgr")
            },
        ),
    ];
    if repaired {
        files.push((
            "app/src/missing.kgr",
            include_str!("../../tests/fixtures/tooling/missing.kgr"),
        ));
    }
    let cancel = CancellationToken::default();
    VirtualSources::new(
        files
            .into_iter()
            .map(|(name, text)| (name.into(), Arc::from(text))),
        &cancel,
    )
    .unwrap()
}

fn project(repaired: bool, changed: bool) -> ProjectDiscovery {
    let cancel = CancellationToken::default();
    let sources = sources(repaired, changed);
    ProjectDiscovery::load(
        &sources,
        "",
        &ProjectOptions {
            selection: MemberSelection::Workspace,
            ..ProjectOptions::default()
        },
        &cancel,
    )
    .unwrap()
}

struct AliasedSources(VirtualSources);

impl SourceInput for AliasedSources {
    fn read(
        &self,
        path: &str,
        max_bytes: usize,
        cancel: &CancellationToken,
    ) -> Result<Option<SourceText>, PackageError> {
        let mut source = self.0.read(path, max_bytes, cancel)?;
        if matches!(path, "app/src/lib.kgr" | "api/src/lib.kgr")
            && let Some(source) = &mut source
        {
            source.key = "same-provider-object".into();
        }
        Ok(source)
    }
}

impl InputTree for AliasedSources {
    fn directories(
        &self,
        directory: &str,
        max_entries: usize,
        cancel: &CancellationToken,
    ) -> Result<Vec<String>, PackageError> {
        self.0.directories(directory, max_entries, cancel)
    }
}

#[test]
fn partial_package_discovery_cannot_hide_conflicting_physical_provider_keys() {
    let cancel = CancellationToken::default();
    let project = ProjectDiscovery::load(
        &AliasedSources(sources(false, false)),
        "",
        &ProjectOptions {
            selection: MemberSelection::Workspace,
            ..ProjectOptions::default()
        },
        &cancel,
    )
    .unwrap();
    assert!(project.roots["app"].program.is_none());
    let error = project.tooling_inputs("app", &cancel).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("physical source shared by modules"),
        "{error}"
    );
}

fn source<'a>(snapshot: &'a ToolingSnapshot, key: &str) -> (FileId, &'a SourceOrigin) {
    snapshot
        .origins
        .iter()
        .find(|(_, origin)| origin.provider_key == key)
        .map(|(id, origin)| (*id, origin))
        .unwrap()
}

#[test]
fn recovering_projection_keeps_exact_sources_scopes_and_atomic_revisions_without_compile_admission()
{
    fn assert_send<T: Send>() {}
    assert_send::<ToolingInputs>();
    assert_send::<ToolingSnapshot>();
    let cancel = CancellationToken::default();
    let discovery = project(false, false);
    let inputs = discovery.tooling_inputs("app", &cancel).unwrap();
    assert!(!inputs.complete());
    assert!(
        !inputs.discovery_complete(),
        "the declared missing module prevents exhaustive search"
    );
    assert!(
        inputs
            .errors()
            .iter()
            .any(|error| error.to_string().contains("missing module missing"))
    );
    assert!(discovery.roots["app"].clone().into_complete().is_err());
    assert!(discovery.tooling_inputs("api", &cancel).unwrap().complete());
    assert_eq!(inputs.sources().len(), 4);
    assert_eq!(
        inputs.total_bytes(),
        inputs
            .sources()
            .iter()
            .map(|source| source.source.text().len())
            .sum()
    );

    let mut database = SourceDatabase::default();
    let old = inputs.source_snapshot(&mut database, &cancel).unwrap();
    let (app, app_origin) = source(&old, "app/src/lib.kgr");
    let (api, api_origin) = source(&old, "api/src/lib.kgr");
    let (broken, _) = source(&old, "app/src/broken.kgr");
    let retained_app = inputs
        .sources()
        .iter()
        .find(|unit| unit.source.key == "app/src/lib.kgr")
        .unwrap();
    assert_eq!(
        old.sources.file(app).unwrap().content_id(),
        retained_app.source.source.content_id()
    );
    assert_eq!(app_origin.module, *inputs.root());
    assert_eq!(app_origin.display_name, "app/src/lib.kgr");
    assert!(!app_origin.host);
    assert!(app_origin.module.path.is_empty());
    assert_eq!(
        source(&old, "app/src/healthy.kgr").1.module.path,
        ["healthy"]
    );
    assert_eq!(
        old.sources.file(broken).unwrap().text(),
        include_str!("../../tests/fixtures/tooling/broken.kgr")
    );
    assert_eq!(
        old.sources
            .packages()
            .scope(&app_origin.module.package)
            .unwrap()
            .dependencies["api"],
        api_origin.module.package
    );
    assert!(
        !old.origins
            .values()
            .any(|origin| origin.module.path == ["missing"] || origin.module.path == ["inline"])
    );

    let complete = project(true, false).tooling_inputs("app", &cancel).unwrap();
    assert!(complete.complete());
    let repaired = complete.source_snapshot(&mut database, &cancel).unwrap();
    assert_eq!(source(&repaired, "app/src/lib.kgr").0, app);
    assert_eq!(source(&repaired, "app/src/broken.kgr").0, broken);
    assert_eq!(source(&repaired, "api/src/lib.kgr").0, api);
    assert_eq!(
        repaired.sources.file(app).unwrap().revision(),
        old.sources.file(app).unwrap().revision()
    );
    assert!(
        repaired.sources.file(broken).unwrap().revision()
            > old.sources.file(broken).unwrap().revision()
    );
    assert_eq!(
        repaired.sources.file(broken).unwrap().text(),
        include_str!("../../tests/fixtures/tooling/repaired.kgr")
    );
    assert!(
        repaired
            .origins
            .values()
            .any(|origin| origin.module.path == ["missing"])
    );
    assert_eq!(
        old.sources.file(broken).unwrap().text(),
        include_str!("../../tests/fixtures/tooling/broken.kgr")
    );

    let edited = project(true, true).tooling_inputs("app", &cancel).unwrap();
    let cancelled = CancellationToken::default();
    cancelled.cancel();
    assert!(matches!(
        edited.source_snapshot(&mut database, &cancelled),
        Err(PackageError::Cancelled)
    ));
    assert_eq!(database.snapshot().revision(), repaired.sources.revision());
    assert_eq!(
        database.snapshot().file(api).unwrap().text(),
        include_str!("../../tests/fixtures/tooling/api.kgr")
    );
    let next = edited.source_snapshot(&mut database, &cancel).unwrap();
    assert_eq!(source(&next, "api/src/lib.kgr").0, api);
    assert!(
        next.sources.file(api).unwrap().revision() > repaired.sources.file(api).unwrap().revision()
    );
    assert_eq!(
        next.sources.file(api).unwrap().text(),
        include_str!("../../tests/fixtures/tooling/api_changed.kgr")
    );
    assert_eq!(
        repaired.sources.file(api).unwrap().text(),
        include_str!("../../tests/fixtures/tooling/api.kgr")
    );
}

#[test]
fn standalone_module_projection_keeps_the_supplied_family_without_package_imports() {
    let cancel = CancellationToken::default();
    let inputs = VirtualSources::new(
        [
            (
                "lib.kgr".into(),
                Arc::from(include_str!("../../tests/fixtures/tooling/app.kgr")),
            ),
            (
                "healthy.kgr".into(),
                Arc::from(include_str!("../../tests/fixtures/tooling/healthy.kgr")),
            ),
        ],
        &cancel,
    )
    .unwrap();
    let discovery =
        ModuleDiscovery::load(&inputs, "lib.kgr", ModuleLimits::default(), &cancel).unwrap();
    let root = ModuleIdentity::single_file("file:///approved/lib.kgr");
    let inputs = ToolingInputs::from_modules(&discovery, root.clone(), &cancel).unwrap();
    assert!(!inputs.complete());
    assert!(!inputs.discovery_complete());
    assert_eq!(inputs.root(), &root);
    assert!(inputs.environment().scopes().is_empty());
    assert_eq!(inputs.sources().len(), 2);
    assert!(
        inputs
            .sources()
            .iter()
            .all(|source| source.module.package == root.package)
    );
    assert_eq!(inputs.sources()[1].module.path, ["healthy"]);
}

#[test]
fn recovered_body_syntax_does_not_hide_complete_declaration_discovery() {
    let cancel = CancellationToken::default();
    let sources = VirtualSources::new(
        [(
            "lib.kgr".into(),
            Arc::from(include_str!(
                "../../tests/fixtures/tooling/unfinished-expression.kgr"
            )),
        )],
        &cancel,
    )
    .unwrap();
    let discovery =
        ModuleDiscovery::load(&sources, "lib.kgr", ModuleLimits::default(), &cancel).unwrap();
    assert!(
        discovery.clone().into_complete().is_err(),
        "compile admission still rejects incomplete syntax"
    );
    let inputs =
        ToolingInputs::from_modules(&discovery, ModuleIdentity::single_file("lib.kgr"), &cancel)
            .unwrap();
    assert!(!inputs.complete());
    assert!(
        inputs.discovery_complete(),
        "every declared module is present despite an unfinished expression"
    );
    assert_eq!(inputs.sources().len(), 1);
    assert_eq!(
        inputs.sources()[0].source.text(),
        include_str!("../../tests/fixtures/tooling/unfinished-expression.kgr")
    );
}

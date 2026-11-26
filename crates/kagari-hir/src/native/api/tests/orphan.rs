//! The trait's declaration identity, not its frontend representation, decides locality.
use crate::{analysis::AnalysisDatabase, imports::environment::ImportEnvironment};
use kagari_common::identity::{
    DefinitionKind, ModuleIdentity, PackageId, map::DefinitionContext, metadata::DefinitionMetadata,
};
use kagari_source::{
    diagnostic::DiagnosticKind,
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_types::{
    declaration::{TraitDef, module::ModuleDecl},
    ty::GenericParam,
};
use std::sync::Arc;

#[test]
fn trait_package_and_ordered_inputs_control_orphans_on_both_routes() {
    let api = ModuleIdentity {
        package: PackageId("game".into()),
        path: vec!["api".into()],
    };
    let context = DefinitionContext::new().unwrap();
    let owner = context
        .intern_child(
            context.intern_root(&api).unwrap(),
            DefinitionKind::Trait,
            "Convert",
            0,
        )
        .unwrap();
    let mut registration = ModuleDecl::new(api.clone());
    registration.traits.push(TraitDef {
        name: "Convert".into(),
        generic_params: vec![GenericParam { owner, position: 0 }],
        associated_types: vec![],
        associated_consts: vec![],
        bounds: vec![],
        supertraits: vec![],
        methods: vec![],
        conversion_adapter: None,
    });
    let registration = Arc::new(
        DefinitionMetadata::checked(context.snapshot(), registration, &Default::default()).unwrap(),
    );
    let cancel = Default::default();
    for registered in [false, true] {
        for (package, header, rejection) in [
            ("game", "impl Convert<bool> for i32 {}", None),
            (
                "plugin",
                "impl Convert<bool> for i32 {}",
                Some("must own the trait or an input type"),
            ),
            ("plugin", "impl Convert<bool> for MyScore {}", None),
            (
                "plugin",
                "impl<T> Convert<MyScore> for T {}",
                Some("uncovered impl parameter"),
            ),
            ("plugin", "impl<T> Convert<MyScore> for [T] {}", None),
            (
                "plugin",
                "impl Convert<bool> for [MyScore] {}",
                Some("must own the trait or an input type"),
            ),
        ] {
            let importer = ModuleIdentity {
                package: PackageId(package.into()),
                path: vec!["compat".into()],
            };
            let mut sources = SourceDatabase::default();
            let mut db = AnalysisDatabase::default();
            if registered {
                db.set_native_modules(vec![registration.clone()]);
            } else {
                sources.bind_module("api.kgr", api.clone()).unwrap();
                sources
                    .set(
                        "api.kgr",
                        "pub trait Convert<S> {}".into(),
                        SourceLayer::Base,
                    )
                    .unwrap();
            }
            let mut imports = ImportEnvironment::default();
            imports
                .modules
                .entry(importer.clone())
                .or_default()
                .dependencies
                .insert(api.clone());
            db.set_import_environment(imports);
            sources.bind_module("compat.kgr", importer.clone()).unwrap();
            let source = format!("use game::api::Convert; struct MyScore {{}} {header}");
            let root = sources
                .set("compat.kgr", source, SourceLayer::Base)
                .unwrap();
            let snapshot = db.snapshot(sources.snapshot(), &cancel).unwrap();
            let file = snapshot.file(root).unwrap();
            let diagnostics = file.result().diagnostics();
            if let Some(reason) = rejection {
                assert!(
                    matches!(diagnostics, [d] if matches!(&d.kind, DiagnosticKind::InvalidTraitImpl { reason: actual, .. } if actual.contains(reason))),
                    "{header}: {diagnostics:?}"
                );
                assert!(snapshot.check_program(root, &cancel).is_err());
            } else {
                assert!(diagnostics.is_empty(), "{header}: {diagnostics:?}");
                snapshot.check_program(root, &cancel).unwrap();
                let header = file
                    .result()
                    .facts()
                    .aggregates
                    .implementations()
                    .find(|item| file.definitions().resolve(item.id).unwrap().module() == &importer)
                    .unwrap();
                assert_eq!(
                    file.definitions()
                        .resolve(header.trait_type.declaration)
                        .unwrap()
                        .module(),
                    &api
                );
            }
        }
    }
}

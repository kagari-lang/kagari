//! Host identity/access semantics must not turn into owning Native objects offline.

use super::{ExternalType, foundation::exported};
use crate::{
    analysis::{AnalysisDatabase, error::AnalysisError},
    types::TypeId,
};
use kagari_common::identity::{
    DefinitionKind, ModuleIdentity, PackageId, map::DefinitionContext, metadata::DefinitionMetadata,
};
use kagari_source::{
    diagnostic::{DiagnosticKind, Severity},
    source_database::{SourceDatabase, SourceLayer, SourceSnapshot},
};
use kagari_types::{
    declaration::{
        FieldDef, TypeDef, TypeDefKind,
        host::{
            HostFieldPolicy, HostReflectionPolicy, HostTypeOwnership, HostTypePolicy, PathAccess,
        },
        module::ModuleDecl,
        path::{HostPathDeclaration, HostPathSegmentDeclaration},
    },
    scalar::BuiltinType,
    ty::Ty,
    visibility::Visibility,
};
use std::sync::Arc;

fn provider() -> Arc<DefinitionMetadata<ModuleDecl>> {
    let context = DefinitionContext::new().unwrap();
    let mut module = ModuleDecl::new(ModuleIdentity {
        package: PackageId("host".into()),
        path: vec![],
    });
    module.types.push(TypeDef {
        name: "Player".into(),
        kind: TypeDefKind::Host(HostTypePolicy {
            ownership: HostTypeOwnership::HostRoot,
            reflection: HostReflectionPolicy::Metadata,
            path_access: PathAccess::ReadWrite,
        }),
        generic_params: vec![],
        bounds: vec![],
        variants: vec![],
        fields: [
            ("hp", true, Visibility::Public, PathAccess::ReadWrite),
            ("rank", false, Visibility::Public, PathAccess::ReadOnly),
            ("hidden", false, Visibility::Private, PathAccess::ReadOnly),
            ("sealed", false, Visibility::Public, PathAccess::None),
        ]
        .into_iter()
        .map(|(name, mutable, visibility, path_access)| FieldDef {
            name: name.into(),
            mutable,
            visibility,
            ty: Ty::Builtin(BuiltinType::I32),
            host_policy: Some(HostFieldPolicy {
                readable: path_access != PathAccess::None,
                path_access,
            }),
        })
        .collect(),
    });
    let definitions = module.collect_definitions(&context).unwrap();
    let root = module
        .definition(DefinitionKind::Struct, "Player", &definitions)
        .unwrap();
    for (name, access) in [
        ("hp", PathAccess::ReadWrite),
        ("rank", PathAccess::ReadOnly),
    ] {
        module.paths.push(HostPathDeclaration {
            root,
            segments: vec![HostPathSegmentDeclaration::Field(
                definitions
                    .lookup_child(root, DefinitionKind::Field, name, 0)
                    .unwrap(),
            )],
            access,
            schema_epoch: 7,
        });
    }
    Arc::new(DefinitionMetadata::checked(definitions, module, &Default::default()).unwrap())
}

#[test]
fn host_exports_preserve_identity_paths_async_and_reference_restrictions() {
    let mut providers = kagari_stdlib::catalog::shared().unwrap();
    providers.push(provider());
    let exports = exported(&providers);
    let mut direct = AnalysisDatabase::new("/project").unwrap();
    direct.set_native_modules(providers);
    let mut offline = AnalysisDatabase::new("/project").unwrap();
    offline.set_external_modules(exports.clone());
    let mut sources = SourceDatabase::new("/project").unwrap();
    for (text, valid) in [
        (
            include_str!("../../../tests/fixtures/analysis/external/host_client.kgr"),
            true,
        ),
        (
            include_str!("../../../tests/fixtures/analysis/external/host_invalid.kgr"),
            false,
        ),
    ] {
        let root = sources
            .set("main.kgr", text.into(), SourceLayer::Overlay)
            .unwrap();
        let direct_snapshot = direct
            .snapshot(sources.snapshot(), &Default::default())
            .unwrap();
        let offline_snapshot = offline
            .snapshot(sources.snapshot(), &Default::default())
            .unwrap();
        let direct_file = direct_snapshot.file(root).unwrap();
        let offline_file = offline_snapshot.file(root).unwrap();
        let errors = |file: &crate::analysis::FileAnalysis| {
            file.result()
                .diagnostics()
                .iter()
                .filter(|diagnostic| diagnostic.severity == Severity::Error)
                .map(|diagnostic| (diagnostic.kind.clone(), diagnostic.span))
                .collect::<Vec<_>>()
        };
        assert_eq!(errors(direct_file), errors(offline_file));
        assert_eq!(
            errors(offline_file).is_empty(),
            valid,
            "{:?}",
            errors(offline_file)
        );
        let facts = offline_file.result().facts();
        let function = facts
            .typed
            .signatures()
            .functions()
            .iter()
            .find(|function| {
                if valid {
                    function.name == "update"
                } else {
                    function.name == "capture"
                }
            })
            .unwrap();
        let TypeId::Host(player) = function.params[0].ty else {
            panic!(
                "Host must not become NativeObject: {:?}",
                function.params[0].ty
            )
        };
        let view = offline_file.definitions().resolve(player).unwrap();
        assert_eq!(view.module().package.0, "host");
        assert_eq!(view.last().unwrap().name, "Player");
        if !valid {
            let diagnostics = offline_file.result().diagnostics();
            assert!(
                diagnostics
                    .iter()
                    .any(|d| matches!(d.kind, DiagnosticKind::InvalidAsyncCapture { .. })),
                "{diagnostics:?}"
            );
            assert!(
                diagnostics
                    .iter()
                    .any(|d| matches!(d.kind, DiagnosticKind::InvalidReference { .. })),
                "{diagnostics:?}"
            );
            assert!(
                diagnostics
                    .iter()
                    .any(|d| matches!(d.kind, DiagnosticKind::InvalidHostPath { .. })),
                "{diagnostics:?}"
            );
        }
    }

    let host_index = exports
        .iter()
        .position(|input| input.types.get("Player") == Some(&ExternalType::Host))
        .unwrap();
    let fingerprint = offline
        .environment_fingerprint(&Default::default())
        .unwrap();
    let mut changed = exports.clone();
    let host = changed[host_index].hosts.get_mut("Player").unwrap();
    let mut record = host.records().clone();
    record.paths[0].schema_epoch += 1;
    *host = Arc::new(
        DefinitionMetadata::checked(host.definitions().clone(), record, &Default::default())
            .unwrap(),
    );
    offline.set_external_modules(changed);
    assert_ne!(
        fingerprint,
        offline
            .environment_fingerprint(&Default::default())
            .unwrap()
    );

    let mut missing = exports.clone();
    missing[host_index].hosts.clear();
    offline.set_external_modules(missing);
    assert!(
        matches!(offline.declarations(SourceSnapshot::default(), &Default::default()), Err(AnalysisError::NativeApi(error)) if error.0.contains("requires schema/path metadata"))
    );
    let mut misclassified = exports.clone();
    misclassified[host_index].types.remove("Player");
    offline.set_external_modules(misclassified);
    assert!(
        matches!(offline.declarations(SourceSnapshot::default(), &Default::default()), Err(AnalysisError::NativeApi(error)) if error.0.contains("requires explicit Host category"))
    );

    // Schema evidence cannot silently override a different parsed field surface.
    let mut mismatch = exports;
    let host = &mut mismatch[host_index];
    let mut altered = SourceDatabase::new("/changed").unwrap();
    altered
        .bind_module("host.kgr", host.source.module_identity().clone())
        .unwrap();
    let id = altered
        .set(
            "host.kgr",
            host.source.text().replace("var hp: i32", "val hp: i32"),
            SourceLayer::Base,
        )
        .unwrap();
    host.source = altered.snapshot().file(id).unwrap().clone();
    offline.set_external_modules(mismatch);
    let rejected = offline
        .snapshot(SourceSnapshot::default(), &Default::default())
        .unwrap();
    assert!(rejected.file(id).unwrap().result().diagnostics().iter().any(|diagnostic| matches!(&diagnostic.kind, DiagnosticKind::InvalidHostPath { reason } if reason.contains("fields differ"))));
}

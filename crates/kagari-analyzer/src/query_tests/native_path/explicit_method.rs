use super::Hosts;
use crate::{AnalyzerQueries, query_tests::render_native};
use kagari_common::span::Span;
use kagari_hir::{
    analysis::AnalysisDatabase, declarations::DeclarationId,
    imports::environment::ImportEnvironment, types::TypeId,
};
use kagari_source::{
    diagnostic::DiagnosticKind,
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_types::{
    declaration::{
        host::{HostPassingStyle, PathAccess},
        path::{
            HostIndexSegmentDeclaration, HostPathDeclaration, HostPathSegmentDeclaration,
            HostVirtualSegmentDeclaration, NativePathOperation, NativePathOperationKind,
        },
    },
    scalar::BuiltinType,
    ty::Ty,
};

const CLIENT: &str = include_str!("../../../tests/fixtures/explicit-path-method.kgr");

#[test]
fn mixed_adapter_exposes_only_its_declared_method() {
    let mut hosts = Hosts::new();
    let access = PathAccess::ReadOnly;
    let scalar = Ty::Builtin(BuiltinType::I32);
    hosts.root(access);
    let method = hosts.method("read_selected", scalar.clone(), HostPassingStyle::Owned);
    let path = HostPathDeclaration {
        root: hosts.owners[0],
        segments: vec![
            HostPathSegmentDeclaration::Index(HostIndexSegmentDeclaration {
                slot: 0,
                collection: Ty::Host(hosts.owners[0]),
                index: scalar.clone(),
                result: Ty::Host(hosts.owners[0]),
                access,
            }),
            HostPathSegmentDeclaration::Virtual(HostVirtualSegmentDeclaration {
                name: "selected".into(),
                result: scalar,
                access,
            }),
        ],
        access,
        schema_epoch: 0,
    };
    hosts.modules[0].paths.push(path.clone());
    hosts.modules[0].path_operations.insert(
        method,
        NativePathOperation {
            path,
            kind: NativePathOperationKind::Read,
        },
    );
    let providers = hosts.providers();
    let document = render_native(&providers[0], &providers);
    let method_path = hosts.context.snapshot().resolve(method).unwrap().to_path();
    let mut sources = SourceDatabase::default();
    let root = sources
        .set("client.kgr", CLIENT.into(), SourceLayer::Base)
        .unwrap();
    let client = sources.snapshot().file(root).unwrap().clone();
    let mut imports = ImportEnvironment::default();
    imports
        .modules
        .entry(client.module_identity().clone())
        .or_default()
        .dependencies
        .insert(hosts.modules[0].identity.clone());
    for from_text in [false, true] {
        let mut db = AnalysisDatabase::default();
        db.set_import_environment(imports.clone());
        let location = if from_text {
            sources
                .bind_module(&document.uri, hosts.modules[0].identity.clone())
                .unwrap();
            let provider = sources
                .set(&document.uri, document.text.clone(), SourceLayer::Base)
                .unwrap();
            let start = document.text.find("fn read_selected(").unwrap() + "fn ".len();
            Some(
                sources
                    .snapshot()
                    .file(provider)
                    .unwrap()
                    .span(Span::new(start, start + "read_selected".len()))
                    .unwrap(),
            )
        } else {
            db.set_native_modules(providers.to_vec());
            None
        };
        let snapshot = db
            .snapshot(sources.snapshot(), &Default::default())
            .unwrap();
        let file = snapshot.file(root).unwrap();
        let queries = file.queries();
        let offset = CLIENT.find("read_selected(").unwrap();
        let target = snapshot.queries().definition_at(root, offset).unwrap();
        let expected = DeclarationId::Definition(file.definitions().lookup(&method_path).unwrap());
        assert_eq!(target.id, expected);
        assert_eq!(target.source_location(), location);
        let signature = queries.call_signature_at(offset).unwrap();
        assert_eq!(signature.declaration, expected);
        assert_eq!(signature.result, TypeId::Builtin(BuiltinType::I32));

        let index = CLIENT.find("item[index]").unwrap();
        let bracket = index + "item".len();
        let virtual_member = CLIENT.find(".selected").unwrap() + 1;
        for offset in [bracket, virtual_member] {
            assert!(queries.field_at(offset).is_none());
            assert!(snapshot.queries().definition_at(root, offset).is_none());
        }
        let argument = snapshot.queries().definition_at(root, bracket + 1).unwrap();
        let parameter = CLIENT.rfind("index: i32").unwrap();
        assert_eq!(
            argument.source_location(),
            client.span(Span::new(parameter, parameter + "index".len()))
        );
        let diagnostics = file.result().diagnostics();
        assert!(
            diagnostics.iter().any(
                |d| matches!(d.kind, DiagnosticKind::InvalidIndexTarget { .. })
                    && d.span == Some(Span::new(index, index + "item[index]".len()))
            ),
            "{diagnostics:?}"
        );
    }
}

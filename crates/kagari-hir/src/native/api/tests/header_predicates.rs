use crate::{analysis::AnalysisDatabase, imports::environment::ImportEnvironment};
use kagari_common::identity::{
    DefinitionKind, ModuleIdentity, PackageId, map::DefinitionContext, metadata::DefinitionMetadata,
};
use kagari_source::{
    diagnostic::DiagnosticKind,
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_types::{
    declaration::{TraitDef, TypeDef, TypeDefKind, module::ModuleDecl},
    ty::{GenericBound, GenericParam, NominalTy, Ty},
};
use std::sync::Arc;

#[test]
fn header_predicates_match_in_structured_and_published_interfaces() {
    let identity = |name: &str| ModuleIdentity {
        package: PackageId("headers".into()),
        path: vec![name.into()],
    };
    let api = identity("api");
    let mut module = ModuleDecl::new(api.clone());
    let context = DefinitionContext::new().unwrap();
    let root = context.intern_root(&api).unwrap();
    let marker = context
        .intern_child(root, DefinitionKind::Trait, "Marker", 0)
        .unwrap();
    let boxed = context
        .intern_child(root, DefinitionKind::Struct, "Box", 0)
        .unwrap();
    module.traits.push(TraitDef {
        name: "Marker".into(),
        generic_params: vec![],
        bounds: vec![],
        methods: vec![],
        supertraits: vec![],
        associated_types: vec![],
        associated_consts: vec![],
        conversion_adapter: None,
    });
    for name in ["Box", "Wrap"] {
        let owner = context
            .intern_child(root, DefinitionKind::Struct, name, 0)
            .unwrap();
        let param = GenericParam { owner, position: 0 };
        let bounds = if owner == boxed {
            vec![]
        } else {
            vec![GenericBound {
                ty: Ty::Struct(NominalTy {
                    declaration: boxed,
                    arguments: vec![param.as_type()],
                    associated_types: Default::default(),
                }),
                constraints: vec![NominalTy {
                    declaration: marker,
                    arguments: vec![],
                    associated_types: Default::default(),
                }],
            }]
        };
        module.types.push(TypeDef {
            name: name.into(),
            kind: TypeDefKind::Struct,
            generic_params: vec![param],
            bounds,
            fields: vec![],
            variants: vec![],
        });
    }
    let cancel = Default::default();
    let definitions = module.collect_definitions(&context).unwrap();
    let provider = Arc::new(DefinitionMetadata::checked(definitions, module, &cancel).unwrap());
    let mut sources = SourceDatabase::default();
    let importer = identity("user");
    let mut imports = ImportEnvironment::default();
    imports
        .modules
        .entry(importer.clone())
        .or_default()
        .dependencies
        .insert(api.clone());
    sources.bind_module("client.kgr", importer).unwrap();
    let client = include_str!("../../../../tests/fixtures/native/header-client.kgr");
    let file = sources
        .set("client.kgr", client.into(), SourceLayer::Base)
        .unwrap();
    let mut direct = AnalysisDatabase::default();
    direct.set_native_modules(vec![provider.clone()]);
    direct.set_import_environment(imports.clone());
    let declarations = direct.declarations(sources.snapshot(), &cancel).unwrap();
    let document = declarations
        .render_native_document(&provider, &cancel)
        .unwrap();
    let structured = direct.snapshot(sources.snapshot(), &cancel).unwrap();
    sources.bind_module("api.kgr", api.clone()).unwrap();
    sources
        .set("api.kgr", document.text, SourceLayer::Base)
        .unwrap();
    let mut editor = AnalysisDatabase::default();
    editor.set_import_environment(imports);
    let published = editor.snapshot(sources.snapshot(), &cancel).unwrap();
    for snapshot in [structured, published] {
        let provider = snapshot
            .files()
            .find(|f| f.source().module_identity() == &api)
            .unwrap();
        assert!(
            provider.result().diagnostics().is_empty(),
            "{:?}",
            provider.result().diagnostics()
        );
        let diagnostics = snapshot.file(file).unwrap().result().diagnostics();
        assert!(
            matches!(diagnostics, [d] if matches!(&d.kind, DiagnosticKind::GenericBoundNotSatisfied { type_name, trait_name } if type_name == "Box<bool>" && trait_name == "Marker")),
            "{diagnostics:?}"
        );
        let span = diagnostics[0].span.unwrap();
        assert_eq!(&client[span.start..span.end], "bad");
    }
}

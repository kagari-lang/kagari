use crate::{
    analysis::AnalysisDatabase,
    hir::{expr::ExprKind, place::PlaceKind},
    imports::environment::ImportEnvironment,
    typeck::table::CallTarget,
    types::TypeId,
};
use kagari_common::identity::{
    DefinitionKind, ModuleIdentity, PackageId, map::DefinitionContext, metadata::DefinitionMetadata,
};
use kagari_source::{
    diagnostic::DiagnosticKind,
    source_database::{SourceDatabase, SourceLayer},
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

const CLIENT: &str = include_str!("../../../../tests/fixtures/native/nested_host_fields.kgr");

fn registration(hp_mutable: bool) -> Arc<DefinitionMetadata<ModuleDecl>> {
    let context = DefinitionContext::new().unwrap();
    let mut module = ModuleDecl::new(ModuleIdentity {
        package: PackageId("host".into()),
        path: vec![],
    });
    let root = context.intern_root(&module.identity).unwrap();
    let stats = context
        .intern_child(root, DefinitionKind::Struct, "Stats", 0)
        .unwrap();
    let field = |name: &str, ty, mutable, visibility| FieldDef {
        name: name.into(),
        ty,
        mutable,
        visibility,
        host_policy: Some(HostFieldPolicy {
            readable: true,
            path_access: PathAccess::ReadOnly,
        }),
    };
    for (name, fields) in [
        (
            "Stats",
            vec![
                field(
                    "hp",
                    Ty::Builtin(BuiltinType::I32),
                    hp_mutable,
                    Visibility::Public,
                ),
                field(
                    "hidden",
                    Ty::Builtin(BuiltinType::I32),
                    true,
                    Visibility::PublicCrate,
                ),
            ],
        ),
        (
            "Player",
            vec![
                field("baseline", Ty::Host(stats), false, Visibility::Public),
                field("current", Ty::Host(stats), true, Visibility::Public),
            ],
        ),
    ] {
        module.types.push(TypeDef {
            name: name.into(),
            kind: TypeDefKind::Host(HostTypePolicy {
                ownership: HostTypeOwnership::HostRoot,
                reflection: HostReflectionPolicy::Metadata,
                path_access: PathAccess::ReadOnly,
            }),
            generic_params: vec![],
            bounds: vec![],
            fields,
            variants: vec![],
        });
    }
    let definitions = module.collect_definitions(&context).unwrap();
    // The same hp declaration is reached through one read-only adapter and
    // one route with no adapter. Neither supplies the Kagari field contract.
    let player = definitions
        .lookup_child(root, DefinitionKind::Struct, "Player", 0)
        .unwrap();
    let segment = |owner, name| {
        HostPathSegmentDeclaration::Field(
            definitions
                .lookup_child(owner, DefinitionKind::Field, name, 0)
                .unwrap(),
        )
    };
    module.paths.push(HostPathDeclaration {
        root: player,
        segments: vec![segment(player, "baseline"), segment(stats, "hp")],
        access: PathAccess::ReadOnly,
        schema_epoch: 0,
    });
    Arc::new(DefinitionMetadata::checked(definitions, module, &Default::default()).unwrap())
}

#[test]
fn host_conversions_check_bodies_and_select_the_applied_method_on_both_routes() {
    let client = include_str!("../../../../tests/fixtures/native/host_conversions.kgr");
    let provider = registration(false);
    let cancel = Default::default();
    let mut direct = AnalysisDatabase::default();
    direct.set_native_modules(vec![provider.clone()]);
    let mut sources = SourceDatabase::default();
    let root = sources
        .set("client.kgr", client.into(), SourceLayer::Base)
        .unwrap();
    let declarations = direct.declarations(sources.snapshot(), &cancel).unwrap();
    let document = declarations
        .render_native_document(&provider, &cancel)
        .unwrap();
    let mut editor = AnalysisDatabase::default();
    editor.set_import_environment(declarations.import_environment().clone());
    let mut documents = SourceDatabase::default();
    let editor_root = documents
        .set("client.kgr", client.into(), SourceLayer::Base)
        .unwrap();
    documents
        .bind_module("host.kgr", provider.records().identity.clone())
        .unwrap();
    documents
        .set("host.kgr", document.text, SourceLayer::Base)
        .unwrap();
    for (db, inputs, root) in [
        (&mut direct, &mut sources, root),
        (&mut editor, &mut documents, editor_root),
    ] {
        let snapshot = db.snapshot(inputs.snapshot(), &cancel).unwrap();
        let file = snapshot.file(root).unwrap();
        let diagnostics = file.result().diagnostics();
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        snapshot.check_program(root, &cancel).unwrap();
        let facts = file.result().facts();
        let hir = &facts.lowered.module;
        let types = facts.typed.type_table();
        for (caller, contract, name) in
            [("read", "TryFrom", "try_from"), ("resolve", "From", "from")]
        {
            let function = hir.functions.iter().find(|f| f.name == caller).unwrap();
            let call = hir
                .block(function.implementation.script_body().unwrap())
                .tail_expr
                .unwrap();
            let resolution = types.call_resolution(call).unwrap();
            let CallTarget::TraitMethod { method, interface } = &resolution.target else {
                panic!("{resolution:?}")
            };
            let defs = file.definitions();
            let owner = defs.resolve(interface.declaration).unwrap();
            assert_eq!(owner.last().unwrap().name, contract);
            assert_eq!(
                Some(*method),
                defs.lookup_child(interface.declaration, DefinitionKind::Method, name, 0)
            );
            if contract == "TryFrom" {
                assert!(matches!(interface.arguments.as_slice(), [TypeId::Host(_)]));
                let error = defs
                    .lookup_child(
                        interface.declaration,
                        DefinitionKind::AssociatedType,
                        "Error",
                        0,
                    )
                    .unwrap();
                let implementation = facts
                    .aggregates
                    .implementations()
                    .find(|item| {
                        item.trait_type.declaration == interface.declaration
                            && item.trait_type.arguments == interface.arguments
                    })
                    .unwrap();
                assert_eq!(
                    implementation.trait_type.associated_types.get(&error),
                    Some(&TypeId::Builtin(BuiltinType::Bool))
                );
                let result = types.expr_type(call).unwrap();
                assert!(
                    matches!(result, TypeId::Enum(result) if result.arguments == [implementation.for_type.clone(), TypeId::Builtin(BuiltinType::Bool)])
                );
            } else {
                assert!(matches!(types.expr_type(call), Some(TypeId::Host(_))));
            }
        }
        inputs
            .set(
                "client.kgr",
                client.replace("        lookup(value)", "        7i32"),
                SourceLayer::Overlay,
            )
            .unwrap();
        let snapshot = db.snapshot(inputs.snapshot(), &cancel).unwrap();
        let diagnostics = snapshot.file(root).unwrap().result().diagnostics();
        assert!(
            matches!(diagnostics, [d] if matches!(&d.kind, DiagnosticKind::ReturnTypeMismatch { function_name, expected, found } if function_name == "from" && expected.ends_with("Player") && found == "i32")),
            "{diagnostics:?}"
        );
    }
}

#[test]
fn nested_host_fields_select_the_declaration_on_both_routes() {
    for hp_mutable in [true, false] {
        let registration = registration(hp_mutable);
        let owner = registration.records().identity.clone();
        let mut sources = SourceDatabase::default();
        let root = sources
            .set("client.kgr", CLIENT.into(), SourceLayer::Base)
            .unwrap();
        let mut imports = ImportEnvironment::default();
        imports
            .modules
            .entry(
                sources
                    .snapshot()
                    .file(root)
                    .unwrap()
                    .module_identity()
                    .clone(),
            )
            .or_default()
            .dependencies
            .insert(owner.clone());
        let cancel = Default::default();
        let mut direct = AnalysisDatabase::default();
        direct.set_native_modules(vec![registration.clone()]);
        direct.set_import_environment(imports.clone());
        let decls = direct.declarations(sources.snapshot(), &cancel).unwrap();
        let document = decls
            .render_native_document(&registration, &cancel)
            .unwrap();
        assert!(document.text.contains("pub(crate) var hidden: i32"));
        let structured = direct.snapshot(sources.snapshot(), &cancel).unwrap();
        sources.bind_module("host.kgr", owner.clone()).unwrap();
        sources
            .set("host.kgr", document.text, SourceLayer::Base)
            .unwrap();
        let mut editor = AnalysisDatabase::default();
        editor.set_import_environment(imports);
        let parsed = editor.snapshot(sources.snapshot(), &cancel).unwrap();
        for snapshot in [structured, parsed] {
            let provider = snapshot
                .files()
                .find(|f| f.source().module_identity() == &owner)
                .unwrap();
            assert!(
                provider.result().diagnostics().is_empty(),
                "{:?}",
                provider.result().diagnostics()
            );
            let file = snapshot.file(root).unwrap();
            let diagnostics = file.result().diagnostics();
            let mut denied = Vec::new();
            for diagnostic in diagnostics {
                let span = diagnostic.span.unwrap();
                let target = &CLIENT[span.start..span.end];
                match (&diagnostic.kind, target) {
                    (DiagnosticKind::InvalidAssignmentTarget { reason }, "player.baseline")
                        if reason == "`val` field `baseline` cannot be assigned" => {}
                    (
                        DiagnosticKind::InvalidAssignmentTarget { reason },
                        "player.baseline.hp" | "player.current.hp",
                    ) if !hp_mutable && reason == "`val` field `hp` cannot be assigned" => {}
                    (DiagnosticKind::UnknownName { name }, "player.current.hidden")
                        if name == "hidden" => {}
                    (
                        DiagnosticKind::InvalidAssignmentTarget { reason },
                        "player.baseline.hidden",
                    ) if reason == "unknown field `hidden`" => {}
                    _ => panic!("unexpected diagnostic at {target}: {diagnostic:?}"),
                }
                denied.push(target);
            }
            let mut expected = vec![
                "player.baseline",
                "player.baseline.hidden",
                "player.current.hidden",
            ];
            if !hp_mutable {
                expected.extend(["player.baseline.hp", "player.current.hp"]);
            }
            expected.sort_unstable();
            denied.sort_unstable();
            assert_eq!(denied, expected);

            let facts = file.result().facts();
            let update = facts
                .lowered
                .module
                .functions
                .iter()
                .find(|f| f.name == "update")
                .unwrap();
            assert_eq!(facts.typed.valid_bodies.contains(&update.id), hp_mutable);
            let types = facts.typed.type_table();
            let nodes = &facts.lowered.module.nodes;
            let definitions = file.definitions();
            let module = definitions.lookup_root(&owner).unwrap();
            let host = |name| {
                definitions
                    .lookup_child(module, DefinitionKind::Struct, name, 0)
                    .unwrap()
            };
            let player = host("Player");
            let stats = host("Stats");
            let selected = |name: &str, field, ty| {
                if name == "hidden" {
                    assert_eq!(field, None);
                    return;
                }
                let (owner, expected_type) = match name {
                    "hp" => (stats, TypeId::Builtin(BuiltinType::I32)),
                    "baseline" | "current" => (player, TypeId::Host(stats)),
                    _ => panic!("unexpected field {name}"),
                };
                let expected = definitions
                    .lookup_child(owner, DefinitionKind::Field, name, 0)
                    .unwrap();
                assert_eq!(field, Some(expected));
                assert_eq!(ty, Some(expected_type));
            };
            for (id, expr) in nodes.expressions() {
                if let ExprKind::Field { name, .. } = &expr.kind {
                    selected(name, types.expr_field(id).copied(), types.expr_type(id));
                }
            }
            for (id, place) in nodes.places() {
                if let PlaceKind::Field { name, .. } = &place.kind {
                    selected(name, types.place_field(id).copied(), types.place_type(id));
                }
            }
        }
    }
}

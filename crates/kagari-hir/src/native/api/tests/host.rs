use crate::{
    analysis::AnalysisDatabase, analyze_source, hir::expr::ExprKind, typeck::table::CallTarget,
    types::TypeId,
};
use kagari_common::identity::{
    DefinitionKind, ModuleIdentity, PackageId, map::DefinitionContext, metadata::DefinitionMetadata,
};
use kagari_source::{
    source::SourceFile,
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_types::{
    callable::CallableImplementation,
    declaration::{
        FnDecl, Param, TypeDef, TypeDefKind,
        host::{
            HostCallPolicy, HostFunctionEffects, HostPassingStyle, HostReflectionPolicy,
            HostTypeOwnership, HostTypePolicy, PathAccess,
        },
        module::ModuleDecl,
    },
    scalar::BuiltinType,
    ty::Ty,
};
use std::{slice, sync::Arc};

#[test]
fn host_bounce_checks_the_same_kagari_call_without_a_passing_policy_on_both_routes() {
    let context = DefinitionContext::new().unwrap();
    let mut module = ModuleDecl::new(ModuleIdentity {
        package: PackageId("example".into()),
        path: vec!["roots".into()],
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
        fields: vec![],
        variants: vec![],
    });
    module.collect_definitions(&context).unwrap();
    let definitions = context.snapshot();
    let player = module
        .definition(DefinitionKind::Struct, "Player", &definitions)
        .unwrap();
    let inspect = context
        .intern_child(
            definitions.lookup_root(&module.identity).unwrap(),
            DefinitionKind::Function,
            "inspect",
            0,
        )
        .unwrap();
    module.functions.push(FnDecl {
        name: "inspect".into(),
        generic_params: vec![],
        bounds: vec![],
        params: vec![Param {
            name: "player".into(),
            ty: Ty::Host(player),
            mutable: false,
        }],
        return_type: Ty::Builtin(BuiltinType::I32),
        implementation: CallableImplementation::Native(inspect),
        method_policy: Default::default(),
        host_policy: Some(HostCallPolicy {
            passing: vec![HostPassingStyle::SharedBorrow],
            effects: HostFunctionEffects::default(),
        }),
    });
    module.collect_definitions(&context).unwrap();
    let module = Arc::new(
        DefinitionMetadata::checked(context.snapshot(), module, &Default::default()).unwrap(),
    );
    let cancel = Default::default();
    let mut sources = SourceDatabase::default();
    let root = sources
        .set(
            "root.kgr",
            include_str!("../../../../tests/fixtures/native/host_bounce.kgr").into(),
            SourceLayer::Base,
        )
        .unwrap();
    let mut structured = AnalysisDatabase::default();
    structured.set_native_modules(vec![module.clone()]);
    let declarations = structured
        .declarations(sources.snapshot(), &cancel)
        .unwrap();
    let view = declarations
        .render_native_document(&module, &cancel)
        .unwrap();
    assert!(view.text.contains("pub native host type Player;"));
    assert!(view.text.contains("player: Player"));
    let direct = structured.snapshot(sources.snapshot(), &cancel).unwrap();
    let mut editor = AnalysisDatabase::default();
    editor.set_import_environment(declarations.import_environment().clone());
    sources
        .bind_module(&view.uri, module.records().identity.clone())
        .unwrap();
    sources
        .set(&view.uri, view.text, SourceLayer::Base)
        .unwrap();
    let parsed = editor.snapshot(sources.snapshot(), &cancel).unwrap();
    for snapshot in [direct, parsed] {
        let file = snapshot.file(root).unwrap();
        let diagnostics = file.result().diagnostics();
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        snapshot.check_program(root, &cancel).unwrap();
        let facts = file.result().facts();
        let bounce = facts
            .typed
            .signatures()
            .functions()
            .iter()
            .find(|f| f.name == "bounce")
            .unwrap();
        let player = &bounce.params[0].ty;
        let TypeId::Host(id) = player else {
            panic!("unexpected Player representation {player:?}");
        };
        let identity = file.definitions().resolve(*id).unwrap();
        assert_eq!(identity.module(), &module.records().identity);
        assert_eq!(identity.last().unwrap().name, "Player");
        assert_eq!(identity.last().unwrap().kind, DefinitionKind::Struct);
        let types = facts.typed.type_table();
        let (site, call) = facts
            .lowered
            .module
            .nodes
            .expressions()
            .find_map(|(id, expr)| {
                let ExprKind::Call { .. } = expr.kind else {
                    return None;
                };
                let call = types.call_resolution(id)?;
                (call.target == CallTarget::Function(bounce.id)).then_some((id, call))
            })
            .expect("resolved local Native bounce call");
        let signature = call.signature.as_ref().unwrap();
        assert_eq!(signature.params.as_slice(), slice::from_ref(player));
        assert_eq!(&signature.return_type, player);
        assert_eq!(types.expr_type(site).as_ref(), Some(player));
    }
    let analysis = analyze_source(
        &SourceFile::new(
            "root-reject.kgr",
            "use example::roots::Player; fn reject(player: Player) { player.unregistered(); }",
        ),
        vec![module],
    )
    .unwrap();
    assert!(!analysis.diagnostics().is_empty());
}

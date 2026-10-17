use crate::{analyze_source, native::render::declaration_source, types::TypeId};
use kagari_common::identity::{
    DefinitionKind, ModuleIdentity, PackageId, map::DefinitionContext, metadata::DefinitionMetadata,
};
use kagari_source::source::SourceFile;
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
use std::sync::Arc;

#[test]
fn shared_host_declarations_keep_external_identity_without_rust_reference_admission() {
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
    let mut providers = kagari_stdlib::catalog::shared().unwrap();
    providers.push(module.clone());
    let view = declaration_source(&module, &providers).unwrap();
    assert!(view.text.contains("pub type Player;"));
    assert!(view.text.contains("player: Player"));
    let analysis = analyze_source(&SourceFile::new("root.kgr", "use example::roots::{Player, inspect}; fn forward(player: Player) -> i32 { inspect(player) }"), providers.clone()).unwrap();
    assert!(
        analysis.diagnostics().is_empty(),
        "{:?}",
        analysis.diagnostics()
    );
    let function = analysis
        .facts
        .typed
        .functions
        .iter()
        .find(|function| function.name == "forward")
        .unwrap();
    assert!(matches!(function.params[0].ty, TypeId::Host(_)));
    for body in [
        "fn reject(player: Ref<Player>) {}",
        "fn reject(player: RefMut<Player>) {}",
        "fn reject(player: Player) { player.unregistered(); }",
    ] {
        let analysis = analyze_source(
            &SourceFile::new(
                "root-reject.kgr",
                format!("use example::roots::Player; {body}"),
            ),
            providers.clone(),
        )
        .unwrap();
        assert!(!analysis.diagnostics().is_empty(), "{body}");
    }
}

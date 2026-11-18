//! Complete embedded definitions are source-owned, not reconstructed default stubs.

use crate::{
    hir::{expr::ExprKind, item::function::FunctionImplementation},
    native::{api::preparation::PreparedRegistration, render::DeclarationSource},
    resolver::collect::resolve_names,
    tests::common::check_module,
};
use kagari_common::identity::{
    DefinitionKind, ModuleIdentity, PackageId, map::DefinitionContext, metadata::DefinitionMetadata,
};
use kagari_types::{
    callable::CallableImplementation,
    declaration::{
        FieldDef, FnDecl, Param, TraitDef, TypeDef, TypeDefKind,
        module::{ImplDecl, ModuleDecl},
    },
    scalar::BuiltinType,
    ty::{NominalTy, Ty},
    visibility::Visibility,
};
use std::sync::Arc;

#[test]
fn embedded_script_definitions_keep_bodies_fields_and_native_binding_separate() {
    let context = DefinitionContext::new().unwrap();
    let mut module = ModuleDecl::new(ModuleIdentity {
        package: PackageId("embedded".into()),
        path: vec!["api".into()],
    });
    let i32_ty = Ty::Builtin(BuiltinType::I32);
    module.types.push(TypeDef {
        name: "Point".into(),
        kind: TypeDefKind::Struct,
        generic_params: vec![],
        bounds: vec![],
        variants: vec![],
        fields: vec![FieldDef {
            name: "value".into(),
            ty: i32_ty.clone(),
            visibility: Visibility::Public,
            mutable: true,
            host_policy: None,
        }],
    });
    module.traits.push(TraitDef {
        name: "Getter".into(),
        generic_params: vec![],
        bounds: vec![],
        supertraits: vec![],
        methods: vec![],
        associated_types: vec![],
        associated_consts: vec![],
        conversion_adapter: None,
    });
    module.collect_definitions(&context).unwrap();
    let definitions = context.snapshot();
    let getter = module
        .definition(DefinitionKind::Trait, "Getter", &definitions)
        .unwrap();
    let point = module
        .definition(DefinitionKind::Struct, "Point", &definitions)
        .unwrap();
    let signature = |name: &str, parameter: &str, ty: Ty, implementation| FnDecl {
        name: name.into(),
        params: vec![Param {
            name: parameter.into(),
            ty,
            mutable: false,
        }],
        return_type: i32_ty.clone(),
        implementation,
        generic_params: vec![],
        bounds: vec![],
        method_policy: Default::default(),
        host_policy: None,
    };
    module.traits[0].methods.push(signature(
        "get",
        "self",
        Ty::SelfType(getter),
        CallableImplementation::Script,
    ));
    module.implementations.push(ImplDecl {
        generic_params: vec![],
        bounds: vec![],
        trait_type: Some(NominalTy {
            declaration: getter,
            arguments: vec![],
            associated_types: Default::default(),
        }),
        for_type: Ty::Struct(NominalTy {
            declaration: point,
            arguments: vec![],
            associated_types: Default::default(),
        }),
        methods: vec![signature(
            "get",
            "self",
            Ty::Struct(NominalTy {
                declaration: point,
                arguments: vec![],
                associated_types: Default::default(),
            }),
            CallableImplementation::Script,
        )],
    });
    module.functions.push(signature(
        "read",
        "value",
        Ty::Struct(NominalTy {
            declaration: point,
            arguments: vec![],
            associated_types: Default::default(),
        }),
        CallableImplementation::Script,
    ));
    let native_echo = context
        .intern_child(
            definitions.lookup_root(&module.identity).unwrap(),
            DefinitionKind::Function,
            "native_echo",
            0,
        )
        .unwrap();
    module.functions.push(signature(
        "native_echo",
        "value",
        i32_ty.clone(),
        CallableImplementation::Native(native_echo),
    ));
    module.collect_definitions(&context).unwrap();
    let metadata = Arc::new(
        DefinitionMetadata::checked(context.snapshot(), module, &Default::default()).unwrap(),
    );
    let source = DeclarationSource {
        uri: "kagari://embedded/api.kgr".into(),
        text: include_str!("../../../../tests/fixtures/native/embedded-script.kgr").into(),
        sites: Default::default(),
        definitions: metadata.definitions().clone(),
    };
    let providers: Arc<[_]> = vec![metadata.clone()].into();
    let without_body = PreparedRegistration::new(
        metadata.clone(),
        providers.clone(),
        None,
        "/embedded",
        Default::default(),
        &Default::default(),
    )
    .unwrap();
    assert!(
        without_body
            .attach(&Default::default())
            .unwrap_err()
            .0
            .contains("requires a complete body")
    );
    let prepared = PreparedRegistration::new(
        metadata,
        providers,
        Some(&source),
        "/embedded",
        Default::default(),
        &Default::default(),
    )
    .unwrap();
    let lowered = prepared.attach(&Default::default()).unwrap();
    assert_eq!(lowered.source_surface().unwrap().text(), source.text);
    assert!(lowered.source_surface().is_some());
    assert_eq!(lowered.module.structs[0].fields.len(), 1);
    assert!(lowered.module.traits[0].methods[0].has_default);
    let get = lowered.module.traits[0].methods[0].function;
    let body = lowered.module.functions[get.index()]
        .implementation
        .script_body()
        .unwrap();
    assert!(matches!(
        lowered
            .module
            .expr(lowered.module.block(body).tail_expr.unwrap())
            .kind,
        ExprKind::Literal(_)
    ));
    let read = lowered
        .module
        .functions
        .iter()
        .find(|function| function.name == "read")
        .unwrap();
    let body = read.implementation.script_body().unwrap();
    assert!(matches!(
        lowered
            .module
            .expr(lowered.module.block(body).tail_expr.unwrap())
            .kind,
        ExprKind::Binary { .. }
    ));
    let method = lowered.module.impls[0].methods[0].function;
    let body = lowered.module.functions[method.index()]
        .implementation
        .script_body()
        .unwrap();
    assert!(matches!(
        lowered
            .module
            .expr(lowered.module.block(body).tail_expr.unwrap())
            .kind,
        ExprKind::Field { .. }
    ));
    let natives = lowered
        .module
        .functions
        .iter()
        .filter(|function| matches!(function.implementation, FunctionImplementation::Native(_)))
        .collect::<Vec<_>>();
    assert_eq!(natives.len(), 1);
    let native = natives[0].id;
    assert!(matches!(
        lowered.module.functions[native.index()].implementation,
        FunctionImplementation::Native(_)
    ));
    let names = resolve_names(&lowered).unwrap().into_checked().unwrap();
    let checked = check_module(&context, &lowered, &names, None);
    assert!(
        checked.diagnostics().is_empty(),
        "{:?}",
        checked.diagnostics()
    );
}

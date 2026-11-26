//! Shared offline Native declarations for semantic and editor tests.
use kagari_common::identity::{
    DefinitionKind, ModuleIdentity, PackageId, map::DefinitionContext, metadata::DefinitionMetadata,
};
use kagari_contract::language::primitive;
use kagari_stdlib::{identity as library, namespaces};
use kagari_types::{
    callable::{CallableImplementation, MethodPolicy},
    declaration::{
        FnDecl, Param, TraitDef,
        host::{HostCallPolicy, HostPassingStyle},
        module::ModuleDecl,
        script::ScriptExpr,
    },
    language::{self, Protocol},
    scalar::BuiltinType,
    ty::{GenericBound, GenericParam, NominalTy, Ty},
};
use std::sync::Arc;

pub(crate) fn module() -> Arc<DefinitionMetadata<ModuleDecl>> {
    let context = fixture_context();
    let mut module = ModuleDecl::new(ModuleIdentity {
        package: PackageId("demo".into()),
        path: vec!["bindings".into()],
    });
    let root = context.intern_root(&module.identity).unwrap();
    module
        .dependencies
        .insert(language::identity(kagari_types::language::Protocol::Add).module);
    for (name, arity) in [("choose", 3), ("echo", 1)] {
        let id = context
            .intern_child(root, DefinitionKind::Function, name, 0)
            .unwrap();
        let parameter = GenericParam {
            owner: id,
            position: 0,
        };
        module
            .documentation
            .insert(id, format!("Registered generic {name} function."));
        module.functions.push(FnDecl {
            host_policy: None,
            name: name.into(),
            implementation: CallableImplementation::Native(id),
            method_policy: MethodPolicy::default(),
            generic_params: vec![parameter.clone()],
            bounds: if name == "choose" {
                vec![GenericBound {
                    ty: parameter.as_type(),
                    constraints: vec![
                        primitive::applied(Protocol::Hash, vec![], &context.snapshot()).unwrap(),
                    ],
                }]
            } else {
                vec![]
            },
            params: (0..arity)
                .map(|i| Param {
                    name: format!("value{i}"),
                    ty: parameter.as_type(),
                    mutable: false,
                })
                .collect(),
            return_type: parameter.as_type(),
        });
    }
    let owner = context
        .intern_child(root, DefinitionKind::Trait, "NativeRead", 0)
        .unwrap();
    let interface = NominalTy {
        declaration: owner,
        arguments: vec![],
        associated_types: Default::default(),
    };
    let mut methods = Vec::new();
    for name in ["read", "fixed"] {
        let template = context
            .intern_child(
                root,
                DefinitionKind::Function,
                &format!("default_{name}"),
                0,
            )
            .unwrap();
        let parameter = GenericParam {
            owner: template,
            position: 0,
        };
        let body = FnDecl {
            host_policy: None,
            name: format!("default_{name}"),
            implementation: CallableImplementation::Native(template),
            method_policy: MethodPolicy::default(),
            generic_params: vec![parameter.clone()],
            bounds: vec![GenericBound {
                ty: parameter.as_type(),
                constraints: vec![interface.clone()],
            }],
            params: vec![Param {
                name: "value".into(),
                ty: parameter.as_type(),
                mutable: false,
            }],
            return_type: Ty::Builtin(BuiltinType::I32),
        };
        let mut method = body.clone();
        method.name = name.into();
        method.generic_params.clear();
        method.bounds.clear();
        method.params[0].name = "self".into();
        method.params[0].ty = Ty::SelfType(owner);
        method.implementation = CallableImplementation::Script;
        let method_id = context
            .intern_child(owner, DefinitionKind::Method, name, 0)
            .unwrap();
        module.script_bodies.insert(
            method_id,
            ScriptExpr::Call {
                callee: Box::new(ScriptExpr::Definition(template)),
                type_arguments: vec![Ty::SelfType(owner)],
                arguments: vec![ScriptExpr::Name("self".into())],
            },
        );
        module.private_functions.insert(template);
        module.documentation.insert(
            context
                .intern_child(owner, DefinitionKind::Method, name, 0)
                .unwrap(),
            format!("Native {name} contract."),
        );
        module.functions.push(body);
        methods.push(method);
    }
    module.traits.push(TraitDef {
        conversion_adapter: None,
        name: "NativeRead".into(),
        generic_params: vec![],
        bounds: vec![],
        supertraits: vec![],
        associated_types: vec![],
        associated_consts: vec![],
        methods,
    });
    let definitions = module.collect_definitions(&context).unwrap();
    module
        .validate(
            &|ty| namespaces::receiver_owner(ty, &definitions),
            &definitions,
        )
        .unwrap();
    Arc::new(DefinitionMetadata::checked(definitions, module, &Default::default()).unwrap())
}

pub(crate) fn function_module(
    identity: ModuleIdentity,
    name: &str,
    parameters: Vec<(&str, Ty)>,
    result: Ty,
) -> Arc<DefinitionMetadata<ModuleDecl>> {
    let context = DefinitionContext::new().unwrap();
    let root = context.intern_root(&identity).unwrap();
    let function = context
        .intern_child(root, DefinitionKind::Function, name, 0)
        .unwrap();
    let mut module = ModuleDecl::new(identity);
    module.functions.push(FnDecl {
        name: name.into(),
        implementation: CallableImplementation::Native(function),
        method_policy: MethodPolicy::default(),
        generic_params: vec![],
        bounds: vec![],
        host_policy: Some(HostCallPolicy {
            passing: vec![HostPassingStyle::Owned; parameters.len()],
            effects: Default::default(),
        }),
        params: parameters
            .into_iter()
            .map(|(name, ty)| Param {
                name: name.into(),
                ty,
                mutable: false,
            })
            .collect(),
        return_type: result,
    });
    module.collect_definitions(&context).unwrap();
    Arc::new(DefinitionMetadata::checked(context.snapshot(), module, &Default::default()).unwrap())
}

pub(crate) fn fixture_context() -> DefinitionContext {
    let context = DefinitionContext::new().unwrap();
    context.intern(&language::identity(Protocol::Hash)).unwrap();
    let iterable = context
        .intern(&language::identity(Protocol::Iterable))
        .unwrap();
    context
        .intern_child(iterable, DefinitionKind::AssociatedType, "Item", 0)
        .unwrap();
    for name in ["List", "MutableList"] {
        context.intern(&library::trait_id(name)).unwrap();
    }
    context
}

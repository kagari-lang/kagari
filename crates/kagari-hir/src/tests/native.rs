//! Application-owned declaration fixtures; no runtime or bundled-library implementation.
use crate::{analysis::AnalysisDatabase, native::render::declaration_source, tests::test_analysis};
use kagari_common::identity::{
    DefinitionKind, ModuleIdentity, PackageId, map::DefinitionContext, metadata::DefinitionMetadata,
};
use kagari_contract::language::primitive;
use kagari_stdlib::{catalog as foundation_catalog, identity as library, namespaces};
use kagari_types::{
    callable::{CallableImplementation, MethodPolicy, NativeDefaultApplication},
    declaration::{
        FnDecl, Param, TraitDef,
        module::{ImplDecl, ModuleDecl},
    },
    language,
    language::Protocol,
    scalar::BuiltinType,
    ty::{Constraint, GenericBound, GenericParam, NominalTy, Ty},
};
use std::sync::Arc;

pub(crate) fn module() -> Arc<DefinitionMetadata<ModuleDecl>> {
    let context = fixture_context();
    let mut module = ModuleDecl::new(ModuleIdentity {
        package: PackageId("demo".into()),
        path: vec!["native".into()],
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
            owner: id.clone(),
            position: 0,
        };
        module
            .documentation
            .insert(id.clone(), format!("Registered generic {name} function."));
        module.functions.push(FnDecl {
            name: name.into(),
            implementation: CallableImplementation::Native(id),
            method_policy: MethodPolicy::default(),
            generic_params: vec![parameter.clone()],
            bounds: if name == "choose" {
                vec![GenericBound {
                    ty: parameter.as_type(),
                    constraints: vec![Constraint::Trait(
                        primitive::applied(Protocol::Hash, vec![], &context.snapshot()).unwrap(),
                    )],
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
        declaration: owner.clone(),
        arguments: vec![],
        associated_types: Default::default(),
    };
    let mut methods = Vec::new();
    for (name, overridable) in [("read", true), ("fixed", false)] {
        let template = context
            .intern_child(
                root,
                DefinitionKind::Function,
                &format!("default_{name}"),
                0,
            )
            .unwrap();
        let parameter = GenericParam {
            owner: template.clone(),
            position: 0,
        };
        let body = FnDecl {
            name: format!("default_{name}"),
            implementation: CallableImplementation::Native(template.clone()),
            method_policy: MethodPolicy::default(),
            generic_params: vec![parameter.clone()],
            bounds: vec![GenericBound {
                ty: parameter.as_type(),
                constraints: vec![Constraint::Trait(interface.clone())],
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
        method.params[0].ty = Ty::SelfType(owner.clone());
        method.method_policy.override_allowed = overridable;
        method.implementation = CallableImplementation::NativeDefault(NativeDefaultApplication {
            declaration: template.clone(),
            arguments: vec![Ty::SelfType(owner.clone())],
        });
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

pub(crate) fn database() -> AnalysisDatabase {
    let mut database = test_analysis();
    database.set_native_modules(
        kagari_stdlib::catalog::shared()
            .unwrap()
            .into_iter()
            .chain([module()])
            .collect(),
    );
    database
}

#[test]
fn native_fixture_source_is_a_valid_offline_declaration() {
    for module in [module(), text_items_module()] {
        let generated =
            declaration_source(&module, &foundation_catalog::shared().unwrap()).unwrap();
        let source = kagari_source::source::SourceFile::new(&generated.uri, &generated.text);
        let parsed = kagari_syntax::parser::parse_declarations(
            &source,
            Default::default(),
            &Default::default(),
        )
        .unwrap();
        assert!(
            parsed.diagnostics().is_empty(),
            "{:?}\n{}",
            parsed.diagnostics(),
            generated.text
        );
    }
}

/// A constrained native trait method can name its receiver in a predicate.
/// Registration owns the contract; generated source is only its presentation.
pub(crate) fn text_items_module() -> Arc<DefinitionMetadata<ModuleDecl>> {
    let context = fixture_context();
    let mut module = ModuleDecl::new(ModuleIdentity {
        package: PackageId("demo".into()),
        path: vec!["text_items".into()],
    });
    let root = context.intern_root(&module.identity).unwrap();
    module
        .dependencies
        .insert(language::identity(kagari_types::language::Protocol::Add).module);
    let owner = context
        .intern_child(root, DefinitionKind::Trait, "TextItems", 0)
        .unwrap();
    let interface = NominalTy {
        declaration: owner.clone(),
        arguments: vec![],
        associated_types: Default::default(),
    };
    let mut required = primitive::applied(Protocol::Iterable, vec![], &context.snapshot()).unwrap();
    required.associated_types.insert(
        context
            .snapshot()
            .lookup_child(
                required.declaration,
                DefinitionKind::AssociatedType,
                "Item",
                0,
            )
            .unwrap(),
        Ty::Builtin(BuiltinType::String),
    );
    module.traits.push(TraitDef {
        conversion_adapter: None,
        name: "TextItems".into(),
        generic_params: vec![],
        bounds: vec![],
        supertraits: vec![],
        associated_types: vec![],
        associated_consts: vec![],
        methods: vec![FnDecl {
            name: "text_items".into(),
            implementation: CallableImplementation::Required,
            method_policy: MethodPolicy::default(),
            generic_params: vec![],
            bounds: vec![GenericBound {
                ty: Ty::SelfType(owner.clone()),
                constraints: vec![Constraint::Trait(required.clone())],
            }],
            params: vec![Param {
                name: "self".into(),
                ty: Ty::SelfType(owner),
                mutable: false,
            }],
            return_type: Ty::Builtin(BuiltinType::USize),
        }],
    });
    for family in 0..4 {
        let owner = context
            .intern_child(
                root,
                DefinitionKind::Impl,
                "",
                module.implementations.len().try_into().unwrap(),
            )
            .unwrap();
        let parameter = GenericParam {
            owner: owner.clone(),
            position: 0,
        };
        let item = parameter.as_type();
        let receiver = match family {
            0 => Ty::Array(Box::new(item)),
            1 => Ty::Trait(NominalTy {
                declaration: context.lookup(&library::trait_id("List")).unwrap(),
                arguments: vec![item],
                associated_types: Default::default(),
            }),
            2 => Ty::Trait(NominalTy {
                declaration: context.lookup(&library::trait_id("MutableList")).unwrap(),
                arguments: vec![item],
                associated_types: Default::default(),
            }),
            _ => Ty::Iter(Box::new(item)),
        };
        let method = FnDecl {
            name: "text_items".into(),
            implementation: CallableImplementation::Native(
                context
                    .intern_child(owner, DefinitionKind::Method, "text_items", 0)
                    .unwrap(),
            ),
            method_policy: MethodPolicy::default(),
            generic_params: vec![parameter.clone()],
            bounds: vec![GenericBound {
                ty: receiver.clone(),
                constraints: vec![Constraint::Trait(required.clone())],
            }],
            params: vec![Param {
                name: "self".into(),
                ty: receiver.clone(),
                mutable: false,
            }],
            return_type: Ty::Builtin(BuiltinType::USize),
        };
        module.implementations.push(ImplDecl {
            generic_params: vec![parameter],
            bounds: vec![],
            trait_type: Some(interface.clone()),
            for_type: receiver,
            methods: vec![method],
        });
    }
    let definitions = module.collect_definitions(&context).unwrap();
    module
        .validate(
            &|ty| namespaces::receiver_owner(ty, &definitions),
            &definitions,
        )
        .unwrap();
    Arc::new(DefinitionMetadata::checked(definitions, module, &Default::default()).unwrap())
}

// Both fixtures refer to these installed foundation declarations. Import the exact
// source identities into their authoring context before constructing semantic types.
fn fixture_context() -> DefinitionContext {
    let providers = foundation_catalog::shared().unwrap();
    let definitions = providers[0].definitions();
    let iterable = Protocol::Iterable.definition(definitions).unwrap();
    let required = [
        Protocol::Hash.definition(definitions).unwrap(),
        iterable,
        definitions
            .lookup_child(iterable, DefinitionKind::AssociatedType, "Item", 0)
            .unwrap(),
        definitions.lookup(&library::trait_id("List")).unwrap(),
        definitions
            .lookup(&library::trait_id("MutableList"))
            .unwrap(),
    ];
    let context = DefinitionContext::new().unwrap();
    context.import(definitions, required).unwrap();
    context
}

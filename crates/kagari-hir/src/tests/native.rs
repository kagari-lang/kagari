//! Application-owned declaration fixtures; no runtime or bundled-library implementation.
use crate::analysis::AnalysisDatabase;
use kagari_abi::{
    callable::{CallableImplementation, MethodPolicy, NativeDefaultApplication},
    declaration::{ImplDecl, ModuleDecl},
    language::{self, Protocol, primitive},
    scalar::BuiltinType,
    types::{
        AbiType, ConstraintAbi, FunctionAbi, GenericBoundAbi, GenericParameterAbi, NominalAbiType,
        ParameterAbi, TraitAbi,
    },
};
use kagari_common::{
    collection::CollectionAccess,
    identity::{DefinitionKind, ModuleIdentity, PackageId, associated_type_id},
};
use std::sync::Arc;

pub(crate) fn module() -> Arc<ModuleDecl> {
    let mut module = ModuleDecl::new(ModuleIdentity {
        package: PackageId("demo".into()),
        path: vec!["native".into()],
    });
    module.dependencies.insert(language::module_identity());
    for (name, arity) in [("choose", 3), ("echo", 1)] {
        let id = module.definition(DefinitionKind::Function, name);
        let parameter = GenericParameterAbi {
            owner: id.clone(),
            position: 0,
        };
        module
            .documentation
            .insert(id.clone(), format!("Registered generic {name} function."));
        module.functions.push(FunctionAbi {
            name: name.into(),
            implementation: CallableImplementation::Native(id),
            method_policy: MethodPolicy::default(),
            generic_params: vec![parameter.clone()],
            bounds: if name == "choose" {
                vec![GenericBoundAbi {
                    ty: parameter.as_type(),
                    constraints: vec![ConstraintAbi::Trait(primitive::applied(
                        Protocol::Hash,
                        vec![],
                    ))],
                }]
            } else {
                vec![]
            },
            params: (0..arity)
                .map(|i| ParameterAbi {
                    name: format!("value{i}"),
                    ty: parameter.as_type(),
                    mutable: false,
                })
                .collect(),
            return_type: parameter.as_type(),
        });
    }
    let owner = module.definition(DefinitionKind::Trait, "NativeRead");
    let interface = NominalAbiType {
        declaration: owner.clone(),
        arguments: vec![],
        associated_types: Default::default(),
    };
    let mut methods = Vec::new();
    for (name, overridable) in [("read", true), ("fixed", false)] {
        let template = module.definition(DefinitionKind::Function, &format!("default_{name}"));
        let parameter = GenericParameterAbi {
            owner: template.clone(),
            position: 0,
        };
        let body = FunctionAbi {
            name: format!("default_{name}"),
            implementation: CallableImplementation::Native(template.clone()),
            method_policy: MethodPolicy::default(),
            generic_params: vec![parameter.clone()],
            bounds: vec![GenericBoundAbi {
                ty: parameter.as_type(),
                constraints: vec![ConstraintAbi::Trait(interface.clone())],
            }],
            params: vec![ParameterAbi {
                name: "value".into(),
                ty: parameter.as_type(),
                mutable: false,
            }],
            return_type: AbiType::Builtin(BuiltinType::I32),
        };
        let mut method = body.clone();
        method.name = name.into();
        method.generic_params.clear();
        method.bounds.clear();
        method.params[0].name = "self".into();
        method.params[0].ty = AbiType::SelfType(owner.clone());
        method.method_policy.override_allowed = overridable;
        method.implementation = CallableImplementation::NativeDefault(NativeDefaultApplication {
            declaration: template.clone(),
            arguments: vec![AbiType::SelfType(owner.clone())],
        });
        module.private_functions.insert(template);
        module.documentation.insert(
            ModuleDecl::method_id(&owner, name),
            format!("Native {name} contract."),
        );
        module.functions.push(body);
        methods.push(method);
    }
    module.traits.push(TraitAbi {
        name: "NativeRead".into(),
        generic_params: vec![],
        bounds: vec![],
        supertraits: vec![],
        associated_types: vec![],
        associated_consts: vec![],
        methods,
    });
    module.validate().unwrap();
    Arc::new(module)
}

pub(crate) fn database() -> AnalysisDatabase {
    let mut database = AnalysisDatabase::default();
    database.set_native_modules(vec![module()]);
    database
}

#[test]
fn native_fixture_source_is_a_valid_offline_declaration() {
    for module in [module(), text_items_module()] {
        let generated = module.declaration_source().unwrap();
        let source = kagari_common::source::SourceFile::new(&generated.uri, &generated.text);
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

/// A constrained native extension can name its fixed receiver in a predicate.
/// Registration owns the contract; generated source is only its presentation.
pub(crate) fn text_items_module() -> Arc<ModuleDecl> {
    let mut module = ModuleDecl::new(ModuleIdentity {
        package: PackageId("demo".into()),
        path: vec!["text_items".into()],
    });
    module.dependencies.insert(language::module_identity());
    for family in 0..4 {
        let owner = module.implementation_id(module.implementations.len());
        let parameter = GenericParameterAbi {
            owner: owner.clone(),
            position: 0,
        };
        let item = parameter.as_type();
        let receiver = match family {
            0 => AbiType::Array(Box::new(item), CollectionAccess::Mutable),
            1 => AbiType::Trait(primitive::applied(Protocol::List, vec![item])),
            2 => AbiType::Trait(primitive::applied(Protocol::MutableList, vec![item])),
            _ => AbiType::Iter(Box::new(item)),
        };
        let mut required = primitive::applied(Protocol::Iterable, vec![]);
        required.associated_types.insert(
            associated_type_id(&required.declaration, "Item"),
            AbiType::Builtin(BuiltinType::String),
        );
        let method = FunctionAbi {
            name: "text_items".into(),
            implementation: CallableImplementation::Native(ModuleDecl::method_id(
                &owner,
                "text_items",
            )),
            method_policy: MethodPolicy::default(),
            generic_params: vec![parameter.clone()],
            bounds: vec![GenericBoundAbi {
                ty: receiver.clone(),
                constraints: vec![ConstraintAbi::Trait(required)],
            }],
            params: vec![ParameterAbi {
                name: "self".into(),
                ty: receiver.clone(),
                mutable: false,
            }],
            return_type: AbiType::Builtin(BuiltinType::USize),
        };
        module.implementations.push(ImplDecl {
            generic_params: vec![parameter],
            bounds: vec![],
            trait_type: None,
            for_type: receiver,
            methods: vec![method],
        });
    }
    module.validate().unwrap();
    Arc::new(module)
}

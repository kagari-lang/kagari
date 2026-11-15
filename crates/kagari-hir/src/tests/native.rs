//! Application-owned declaration fixtures; no runtime or bundled-library implementation.

use crate::tests::native_fixture::{fixture_context, module};

use crate::{analysis::AnalysisDatabase, native::render::declaration_source, tests::test_analysis};
use kagari_common::identity::{
    DefinitionKind, ModuleIdentity, PackageId, metadata::DefinitionMetadata,
};
use kagari_contract::language::primitive;
use kagari_stdlib::{catalog as foundation_catalog, identity as library, namespaces};
use kagari_syntax::parser::{ParseOptions, parse};
use kagari_types::{
    callable::{CallableImplementation, MethodPolicy},
    declaration::{
        FnDecl, Param, TraitDef,
        module::{ImplDecl, ModuleDecl},
    },
    language,
    language::Protocol,
    scalar::BuiltinType,
    ty::{GenericBound, GenericParam, NominalTy, Ty},
};
use std::sync::Arc;

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
    for module in [module(), text_items_module(false)] {
        let generated =
            declaration_source(&module, &foundation_catalog::shared().unwrap()).unwrap();
        let source = kagari_source::source::SourceFile::new(&generated.uri, &generated.text);
        let parsed = parse(&source, ParseOptions::default(), &Default::default()).unwrap();
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
pub(crate) fn text_items_module(
    interface_implementations: bool,
) -> Arc<DefinitionMetadata<ModuleDecl>> {
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
        declaration: owner,
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
            host_policy: None,
            name: "text_items".into(),
            implementation: CallableImplementation::Required,
            method_policy: MethodPolicy::default(),
            generic_params: vec![],
            bounds: vec![GenericBound {
                ty: Ty::SelfType(owner),
                constraints: vec![required.clone()],
            }],
            params: vec![Param {
                name: "self".into(),
                ty: Ty::SelfType(owner),
                mutable: false,
            }],
            return_type: Ty::Builtin(BuiltinType::U64),
        }],
    });
    for family in 0..3 {
        if !interface_implementations && matches!(family, 1 | 2) {
            continue;
        }
        let owner = context
            .intern_child(
                root,
                DefinitionKind::Impl,
                "",
                module.implementations.len().try_into().unwrap(),
            )
            .unwrap();
        let parameter = GenericParam { owner, position: 0 };
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
            _ => unreachable!("bounded receiver family"),
        };
        let method = FnDecl {
            host_policy: None,
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
                constraints: vec![required.clone()],
            }],
            params: vec![Param {
                name: "self".into(),
                ty: receiver.clone(),
                mutable: false,
            }],
            return_type: Ty::Builtin(BuiltinType::U64),
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

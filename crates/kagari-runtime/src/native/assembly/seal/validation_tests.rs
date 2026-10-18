//! Both publication paths reject the same malformed semantic records, before bindings.
use super::seal;
use crate::native::{
    assembly::draft::DraftState, catalog::DeclarationCatalog, module::NativeModule,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind, ModuleIdentity, map::DefinitionContext, metadata::DefinitionMetadata,
        table::DefinitionId,
    },
};
use kagari_types::{
    declaration::{
        TypeDef, TypeDefKind, VariantDef,
        module::{ImplDecl, ModuleDecl},
        names::{ExportName, NameNamespace},
    },
    ty::{GenericParam, NominalTy, Ty},
};
use std::collections::BTreeMap;

fn fixture() -> (DefinitionContext, DraftState, DefinitionId) {
    let context = DefinitionContext::new().unwrap();
    let mut draft = DraftState::new(ModuleIdentity::single_file("semantic-validation"));
    draft.declaration.types.push(TypeDef {
        name: "Choice".into(),
        kind: TypeDefKind::Enum,
        generic_params: vec![],
        bounds: vec![],
        fields: vec![],
        variants: vec![VariantDef {
            name: "Item".into(),
            payload: vec![],
            reports_failure: false,
        }],
    });
    let definitions = draft.declaration.collect_definitions(&context).unwrap();
    let id = draft
        .declaration
        .definition(DefinitionKind::Enum, "Choice", &definitions)
        .unwrap();
    (context, draft, id)
}

fn assert_rejected(context: &DefinitionContext, draft: DraftState, message: &str) {
    assert_rejected_with_providers(context, draft, &[], message);
}

fn assert_rejected_with_providers(
    context: &DefinitionContext,
    draft: DraftState,
    modules: &[ModuleDecl],
    message: &str,
) {
    let providers = DeclarationCatalog::declared(modules, context).unwrap();
    let native = NativeModule::checked(
        draft.declaration.clone(),
        vec![],
        BTreeMap::new(),
        vec![],
        &providers,
    )
    .expect_err("native preparation must reject the semantic record");
    assert!(native.to_string().contains(message), "{native}");
    let metadata = DefinitionMetadata::checked(
        context.snapshot(),
        draft.declaration.clone(),
        &CancellationToken::default(),
    );
    if let Ok(metadata) = metadata {
        let mut inputs = modules
            .iter()
            .map(|module| {
                DefinitionMetadata::checked(
                    context.snapshot(),
                    module.clone(),
                    &CancellationToken::default(),
                )
                .unwrap()
            })
            .collect::<Vec<_>>();
        inputs.push(metadata);
        let imported = DeclarationCatalog::from_declarations(&inputs)
            .expect_err("declaration catalog must reject the semantic record");
        assert!(imported.to_string().contains(message), "{imported}");
    }
    let sealed = seal(context, &providers, &mut [draft])
        .expect_err("declaration sealing must reject the semantic record");
    assert!(sealed.to_string().contains(message), "{sealed}");
}

#[test]
fn export_paths_must_resolve_to_real_declarations_and_variants() {
    for variant in [false, true] {
        let (context, provider, choice) = fixture();
        let mut draft = DraftState::new(ModuleIdentity::single_file("export-consumer"));
        let target = if variant {
            context
                .intern_child(choice, DefinitionKind::Variant, "Absent", 0)
                .unwrap()
        } else {
            let root = context.intern_root(&provider.declaration.identity).unwrap();
            context
                .intern_child(root, DefinitionKind::Enum, "Absent", 0)
                .unwrap()
        };
        let namespace = if variant {
            NameNamespace::Value
        } else {
            NameNamespace::Type
        };
        draft
            .declaration
            .exports
            .insert(ExportName::new(namespace, "Alias"), target);
        assert_rejected_with_providers(
            &context,
            draft,
            &[provider.declaration],
            "re-export target is not declared by an available provider",
        );
    }
}

#[test]
fn exports_preserve_namespace_and_scope_checks() {
    let (context, provider, choice) = fixture();
    let mut draft = DraftState::new(ModuleIdentity::single_file("export-consumer"));
    let variant = context
        .intern_child(choice, DefinitionKind::Variant, "Item", 0)
        .unwrap();
    draft.declaration.exports.insert(
        ExportName::new(NameNamespace::Type, "WrongNamespace"),
        variant,
    );
    assert_rejected_with_providers(&context, draft, &[provider.declaration], "native API:");

    let (context, mut draft, _) = fixture();
    let foreign = DefinitionContext::new().unwrap();
    let root = foreign.intern_root(&draft.declaration.identity).unwrap();
    let target = foreign
        .intern_child(root, DefinitionKind::Enum, "Choice", 0)
        .unwrap();
    draft
        .declaration
        .exports
        .insert(ExportName::new(NameNamespace::Type, "Foreign"), target);
    assert_rejected(&context, draft, "invalid native identities");
}

#[test]
fn real_type_and_variant_exports_survive_both_projections() {
    let (context, provider, choice) = fixture();
    let mut draft = DraftState::new(ModuleIdentity::single_file("export-consumer"));
    let variant = context
        .intern_child(choice, DefinitionKind::Variant, "Item", 0)
        .unwrap();
    draft
        .declaration
        .exports
        .insert(ExportName::new(NameNamespace::Type, "Alias"), choice);
    draft
        .declaration
        .exports
        .insert(ExportName::new(NameNamespace::Value, "Alias"), variant);
    let providers = DeclarationCatalog::declared([&provider.declaration], &context).unwrap();
    let native = NativeModule::checked(
        draft.declaration.clone(),
        vec![],
        BTreeMap::new(),
        vec![],
        &providers,
    )
    .unwrap();
    let sealed = seal(&context, &providers, &mut [draft]).unwrap();
    let declaration = &sealed.modules()[0];
    assert_eq!(declaration.records().exports, native.declaration().exports);
    assert_eq!(
        declaration
            .records()
            .compiler_input_bytes(declaration.definitions(), &CancellationToken::default())
            .unwrap(),
        native
            .declaration()
            .compiler_input_bytes(native.definitions(), &CancellationToken::default())
            .unwrap()
    );
}

#[test]
fn complete_catalog_checks_nominal_arity_before_binding_preparation() {
    let (context, mut draft, choice) = fixture();
    draft.declaration.types[0]
        .generic_params
        .push(GenericParam {
            owner: choice,
            position: 0,
        });
    let mut container = draft.declaration.types[0].clone();
    container.name = "Container".into();
    container.generic_params.clear();
    container.variants[0].payload.push(Ty::Enum(NominalTy {
        declaration: choice,
        arguments: vec![],
        associated_types: BTreeMap::new(),
    }));
    draft.declaration.types.push(container);
    draft.declaration.collect_definitions(&context).unwrap();
    assert_rejected(&context, draft, "type applications:");
}

#[test]
fn local_shape_and_foreign_inherent_ownership_are_not_weakened() {
    let (context, mut draft, _) = fixture();
    draft
        .declaration
        .types
        .push(draft.declaration.types[0].clone());
    assert_rejected(&context, draft, "invalid or unsupported native declaration");

    let (context, mut draft, _) = fixture();
    let root = context
        .intern_root(&ModuleIdentity::single_file("foreign-owner"))
        .unwrap();
    let foreign = context
        .intern_child(root, DefinitionKind::Enum, "Choice", 0)
        .unwrap();
    draft.declaration.implementations.push(ImplDecl {
        generic_params: vec![],
        bounds: vec![],
        trait_type: None,
        for_type: Ty::Enum(NominalTy {
            declaration: foreign,
            arguments: vec![],
            associated_types: BTreeMap::new(),
        }),
        methods: vec![],
    });
    draft.declaration.collect_definitions(&context).unwrap();
    assert_rejected(&context, draft, "native API:");
}

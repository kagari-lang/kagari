//! Provider module ownership survives catalog projections without inferring owners from references.
use crate::native::{
    assembly::{ModuleDefinition, ModuleSetBuilder},
    catalog::{DeclarationCatalog, import::CatalogImports},
};
use kagari_common::identity::{
    DefinitionKind, DefinitionPath, DefinitionPathSegment, ModuleIdentity, map::DefinitionContext,
    metadata::DefinitionMetadata,
};
use kagari_types::declaration::{
    TraitDef,
    module::ModuleDecl,
    names::{ExportName, NameNamespace},
};

fn owner(name: &str) -> ModuleIdentity {
    ModuleIdentity::single_file(name)
}

fn provider_catalogs() -> Vec<DeclarationCatalog> {
    let mut set = ModuleSetBuilder::new(&DeclarationCatalog::default()).unwrap();
    set.add(ModuleDefinition::new(owner("empty"), |_| Ok(())))
        .unwrap();
    set.add(ModuleDefinition::new(owner("source"), |module| {
        module.define_trait("Marker", 0, |_, _| Ok(()))?;
        Ok(())
    }))
    .unwrap();
    set.add(ModuleDefinition::new(owner("exports"), |module| {
        let reference = module.reference_trait(
            DefinitionPath {
                module: owner("source"),
                path: vec![DefinitionPathSegment {
                    kind: DefinitionKind::Trait,
                    name: "Marker".into(),
                    occurrence: 0,
                }],
            },
            0,
        )?;
        module.export(
            ExportName::new(NameNamespace::Type, "Marker"),
            reference.id(),
        )
    }))
    .unwrap();
    let assembly = set.finish().unwrap();
    let snapshot = assembly.declarations();
    let modules = assembly.into_modules().unwrap();
    vec![
        snapshot.catalog().clone(),
        DeclarationCatalog::from_declarations(snapshot.modules()).unwrap(),
        DeclarationCatalog::from_modules(&modules.iter().collect::<Vec<_>>()).unwrap(),
    ]
}

#[test]
fn explicit_dependencies_and_path_exports_keep_empty_provider_owners() {
    for reverse in [false, true] {
        let mut recipes = vec![
            ModuleDefinition::new(owner("empty"), |_| Ok(())),
            ModuleDefinition::new(owner("contracts"), |module| {
                module.define_trait("Marker", 0, |_, _| Ok(()))?;
                Ok(())
            }),
            ModuleDefinition::new(owner("facade"), |module| {
                module.dependency(owner("empty"))?;
                module.export_path(
                    ExportName::new(NameNamespace::Type, "Marker"),
                    DefinitionPath {
                        module: owner("contracts"),
                        path: vec![DefinitionPathSegment {
                            kind: DefinitionKind::Trait,
                            name: "Marker".into(),
                            occurrence: 0,
                        }],
                    },
                )
            }),
            ModuleDefinition::new(owner("prelude"), |module| {
                module.prelude(true);
                module.dependency(owner("empty"))?;
                module.dependency(owner("facade"))
            }),
        ];
        if reverse {
            recipes.reverse();
        }
        let mut set = ModuleSetBuilder::new(&DeclarationCatalog::default()).unwrap();
        for recipe in recipes {
            set.add(recipe).unwrap();
        }
        let assembly = set.finish().unwrap();
        let snapshot = assembly.declarations();
        for name in ["facade", "prelude"] {
            let module = snapshot
                .modules()
                .iter()
                .find(|module| module.records().identity == owner(name))
                .unwrap();
            assert!(module.records().dependencies.contains(&owner("empty")));
        }
        let modules = assembly.into_modules().unwrap();
        assert!(
            modules
                .iter()
                .find(|module| module.declaration().identity == owner("prelude"))
                .unwrap()
                .declaration()
                .dependencies
                .contains(&owner("empty"))
        );
    }
    let providers = provider_catalogs().remove(0);
    let mut set = ModuleSetBuilder::new(&providers).unwrap();
    set.add(ModuleDefinition::new(owner("consumer"), |module| {
        module.dependency(owner("empty"))
    }))
    .unwrap();
    set.finish().unwrap();
}

#[test]
fn dependency_and_export_paths_do_not_fabricate_providers_or_members() {
    for export in [false, true] {
        let mut set = ModuleSetBuilder::new(&DeclarationCatalog::default()).unwrap();
        set.add(ModuleDefinition::new(owner("consumer"), move |module| {
            assert!(module.dependency(owner("consumer")).is_err());
            if export {
                module.export_path(
                    ExportName::new(NameNamespace::Type, "Absent"),
                    DefinitionPath {
                        module: owner("missing"),
                        path: vec![DefinitionPathSegment {
                            kind: DefinitionKind::Trait,
                            name: "Absent".into(),
                            occurrence: 0,
                        }],
                    },
                )
            } else {
                module.dependency(owner("missing"))
            }
        }))
        .unwrap();
        assert!(set.finish().is_err());
    }
    for namespace in [NameNamespace::Type, NameNamespace::Value] {
        let mut set = ModuleSetBuilder::new(&DeclarationCatalog::default()).unwrap();
        set.add(ModuleDefinition::new(owner("contracts"), |module| {
            module.define_trait("Marker", 0, |_, _| Ok(()))?;
            Ok(())
        }))
        .unwrap();
        set.add(ModuleDefinition::new(owner("facade"), move |module| {
            let path = DefinitionPath {
                module: owner("contracts"),
                path: vec![DefinitionPathSegment {
                    kind: DefinitionKind::Trait,
                    name: "Marker".into(),
                    occurrence: 0,
                }],
            };
            let name = ExportName::new(namespace, "Marker");
            module.export_path(name.clone(), path.clone())?;
            assert!(module.export_path(name, path).is_err());
            Ok(())
        }))
        .unwrap();
        assert_eq!(set.finish().is_ok(), namespace == NameNamespace::Type);
    }
}

fn assert_owner_rejected(providers: &DeclarationCatalog, name: &str) {
    let mut set = ModuleSetBuilder::new(providers).unwrap();
    let error = set
        .add(ModuleDefinition::new(owner(name), |_| {
            panic!("rejected provider recipe must never execute")
        }))
        .expect_err("an external module owner cannot be redefined");
    assert!(error.message().contains("external module owner"), "{error}");
    set.finish().unwrap();
}

fn assert_owner_available(providers: &DeclarationCatalog, name: &str) {
    let mut set = ModuleSetBuilder::new(providers).unwrap();
    set.add(ModuleDefinition::new(owner(name), |_| Ok(())))
        .unwrap();
    set.finish().unwrap();
}

#[test]
fn empty_provider_owner_is_rejected_before_recipe_execution() {
    for catalog in provider_catalogs() {
        assert_owner_rejected(&catalog, "empty");
    }
}

#[test]
fn reexport_only_provider_owner_is_rejected_before_recipe_execution() {
    for catalog in provider_catalogs() {
        assert_owner_rejected(&catalog, "exports");
    }
}

#[test]
fn owner_inventory_merges_and_imports_without_claiming_interned_references() {
    let context = DefinitionContext::new().unwrap();
    let referenced = context.intern_root(&owner("referenced_only")).unwrap();
    let referenced = context
        .intern_child(referenced, DefinitionKind::Trait, "Unprovided", 0)
        .unwrap();
    let mut first = ModuleDecl::new(owner("first"));
    first.trait_references.insert(
        referenced,
        TraitDef {
            name: "Unprovided".into(),
            conversion_adapter: None,
            generic_params: vec![],
            bounds: vec![],
            supertraits: vec![],
            methods: vec![],
            associated_types: vec![],
            associated_consts: vec![],
        },
    );
    let first = DefinitionMetadata::checked(
        first.collect_definitions(&context).unwrap(),
        first,
        &Default::default(),
    )
    .unwrap();
    let original = DeclarationCatalog::from_declarations([&first]).unwrap();
    let other = DeclarationCatalog::declared(
        [&ModuleDecl::new(owner("second"))],
        &DefinitionContext::new().unwrap(),
    )
    .unwrap();
    let mut imports = CatalogImports::new(DefinitionContext::new().unwrap());
    let original_import = imports.import(&original).unwrap();
    assert!(
        original_import
            .definitions()
            .lookup_root(&owner("referenced_only"))
            .is_some(),
        "the reference must survive importing without becoming a provider"
    );
    let mut merged = original.clone();
    merged.merge(&other).unwrap();
    merged.merge(&other).unwrap();
    let merged_import = imports.import(&merged).unwrap();

    for catalog in [&original, &original_import] {
        assert_owner_rejected(catalog, "first");
        assert_owner_available(catalog, "second");
        assert_owner_available(catalog, "referenced_only");
    }
    for catalog in [&merged, &merged_import] {
        assert_owner_rejected(catalog, "first");
        assert_owner_rejected(catalog, "second");
        assert_owner_available(catalog, "referenced_only");
    }
    let foreign = merged_import.foreign_to(&original_import).unwrap();
    assert_owner_available(&foreign, "first");
    assert_owner_rejected(&foreign, "second");
}

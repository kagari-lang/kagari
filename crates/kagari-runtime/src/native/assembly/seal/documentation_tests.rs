//! Batch inheritance uses canonical members, never method spelling or recipe order.
use crate::native::{
    assembly::{
        ModuleDefinition, ModuleSetBuilder,
        authoring::PendingBinding,
        draft::{TraitImplementation, traits::TraitOverride},
    },
    binding::{NativeBinding, NativeResult},
    catalog::DeclarationCatalog,
    declarations::MethodDecl,
    types::Type,
};
use kagari_common::identity::{
    DefinitionKind, DefinitionPath, DefinitionPathSegment, ModuleIdentity, map::DefinitionContext,
};
use kagari_types::declaration::module::ModuleDecl;

fn owner(name: &str) -> ModuleIdentity {
    ModuleIdentity::single_file(name)
}

fn contract_path(name: &str) -> DefinitionPath {
    DefinitionPath {
        module: owner(name),
        path: vec![DefinitionPathSegment {
            kind: DefinitionKind::Trait,
            name: "Read".into(),
            occurrence: 0,
        }],
    }
}

fn contract(name: &'static str, text: &'static str) -> ModuleDefinition {
    ModuleDefinition::new(owner(name), move |module| {
        let reference = module.reference_trait(contract_path(name), 0)?;
        let mut methods = Vec::new();
        for name in ["get", "explicit", "undocumented"] {
            let member = module.reference_method(&reference, name, 0)?;
            methods.push(module.trait_method(
                reference.id(),
                MethodDecl::instance(name).returns(Type::u64()),
            )?);
            if name != "undocumented" {
                module.document(member.id(), text)?;
            }
        }
        module.define_trait("Read", 0, |record, _| {
            record.methods = methods;
            Ok(())
        })?;
        Ok(())
    })
}

fn implementation() -> ModuleDefinition {
    ModuleDefinition::new(owner("implementation"), |module| {
        for name in ["left", "right"] {
            let interface = module.reference_trait(contract_path(name), 0)?;
            let mut entries = Vec::new();
            for member in ["get", "explicit", "undocumented"] {
                let reference = module.reference_method(&interface, member, 0)?;
                let mut entry = TraitOverride::new(
                    reference,
                    PendingBinding::implementation_with_send(|_| -> NativeResult<NativeBinding> {
                        panic!("documentation must not prepare bindings")
                    }),
                );
                if member == "explicit" {
                    entry = entry.documentation("Implementation-specific prose.");
                }
                entries.push(entry);
            }
            module.trait_implementation_with(0, |_| {
                Ok(TraitImplementation {
                    receiver: Type::i32(),
                    interface: interface.apply([])?,
                    bounds: vec![],
                    bindings: vec![],
                }
                .with_overrides(entries))
            })?;
        }
        Ok(())
    })
}

#[test]
fn canonical_member_docs_inherit_in_both_orders_without_overwriting_explicit_prose() {
    for reverse in [false, true] {
        let mut recipes = vec![
            implementation(),
            contract("left", "Left contract prose."),
            contract("right", "Right contract prose."),
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
        let module = snapshot
            .modules()
            .iter()
            .find(|module| module.records().identity == owner("implementation"))
            .unwrap();
        for (index, implementation) in module.records().implementations.iter().enumerate() {
            let interface = implementation.trait_type.as_ref().unwrap();
            let source = snapshot
                .modules()
                .iter()
                .find(|source| {
                    &source.records().identity
                        == module
                            .definitions()
                            .resolve(interface.declaration)
                            .unwrap()
                            .module()
                })
                .unwrap();
            let id = module
                .records()
                .implementation_id(index, module.definitions())
                .unwrap();
            for method in &implementation.methods {
                let target = ModuleDecl::method_id(id, &method.name, module.definitions()).unwrap();
                let origin = ModuleDecl::method_id(
                    interface.declaration,
                    &method.name,
                    module.definitions(),
                )
                .unwrap();
                match method.name.as_str() {
                    "get" => assert_eq!(
                        module.records().documentation[&target],
                        source.records().documentation[&origin]
                    ),
                    "explicit" => assert_eq!(
                        module.records().documentation[&target],
                        "Implementation-specific prose."
                    ),
                    "undocumented" => {
                        assert!(!module.records().documentation.contains_key(&target))
                    }
                    _ => unreachable!(),
                }
            }
        }
    }
}

#[test]
fn authored_documentation_rejects_foreign_scope_and_owner() {
    let foreign = DefinitionContext::new().unwrap();
    let foreign_id = foreign.intern(&contract_path("consumer")).unwrap();
    let mut set = ModuleSetBuilder::new(&DeclarationCatalog::default()).unwrap();
    set.add(contract("left", "Owner prose.")).unwrap();
    set.add(ModuleDefinition::new(owner("consumer"), move |module| {
        module.documentation("Module prose.");
        let local = module.define_trait("Read", 0, |_, _| Ok(()))?;
        module.document(local.id(), "Local prose.")?;
        assert!(module.document(foreign_id, "Foreign scope").is_err());
        let reference = module.reference_trait(contract_path("left"), 0)?;
        assert!(module.document(reference.id(), "Foreign owner").is_err());
        Ok(())
    }))
    .unwrap();
    let assembly = set.finish().unwrap();
    let snapshot = assembly.declarations();
    let module = snapshot
        .modules()
        .iter()
        .find(|module| module.records().identity == owner("consumer"))
        .unwrap();
    assert_eq!(module.records().module_documentation, "Module prose.");
    let id = module
        .records()
        .definition(DefinitionKind::Trait, "Read", module.definitions())
        .unwrap();
    assert_eq!(module.records().documentation[&id], "Local prose.");
}

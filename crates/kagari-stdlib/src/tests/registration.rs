//! Scoped foundation declarations and storage admission require no source compiler.
use crate::{catalog, declarations::StandardDeclarations, namespaces};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionKind, map::DefinitionContext},
};
use kagari_runtime::{
    Runtime,
    native::{
        binding::NativeResult,
        builder::ModuleBuilder,
        module::NativeModule,
        storage::{NativePayload, NativeStorage},
    },
    resource::HeapCharge,
    value::Value,
};
use kagari_types::{
    callable::CallableImplementation,
    declaration::{
        module::ModuleDecl,
        names::{ExportName, NameNamespace},
    },
    ty::Ty,
};

use std::collections::BTreeSet;

#[derive(Debug)]
struct EmptyPayload;

impl NativePayload for EmptyPayload {
    fn trace<'payload>(&'payload self, _visit: &mut dyn FnMut(&'payload Value)) {}

    fn charge(&self) -> NativeResult<HeapCharge> {
        Ok(HeapCharge::new(0, 0))
    }
}

#[test]
fn declaration_storage_binding_preserves_local_contract() {
    let language = StandardDeclarations::default();
    let catalog = language.catalog().unwrap();
    let future = catalog.future_type().unwrap();
    let declaration = language
        .declarations()
        .unwrap()
        .iter()
        .find(|declaration| {
            declaration.records().identity
                == *future.definitions().resolve(future.id()).unwrap().module()
        })
        .unwrap()
        .as_ref()
        .clone();
    let builder = || {
        ModuleBuilder::from_declaration(declaration.clone(), &catalog, Default::default()).unwrap()
    };
    assert!(
        builder().finish().is_err(),
        "declarations alone cannot install payloads"
    );
    let mut module = builder();
    assert!(
        module
            .bind_storage(&future, NativeStorage::payload::<EmptyPayload>())
            .is_err()
    );
    module
        .bind_storage(&future, NativeStorage::future())
        .unwrap();
    assert!(
        module
            .bind_storage(&future, NativeStorage::future())
            .is_err()
    );
    module.finish().unwrap();
    let mut foreign = ModuleBuilder::new("example::foreign_storage", &catalog).unwrap();
    assert!(
        foreign
            .bind_storage(&future, NativeStorage::future())
            .is_err()
    );
    foreign.finish().unwrap();
}

#[test]
fn standard_default_inventory_has_owned_bindings_and_unbounded_elements() {
    let modules = catalog::declarations().unwrap();
    let default = modules
        .iter()
        .find(|module| module.records().identity == namespaces::module("core", "default"))
        .unwrap();
    let default_id = default
        .records()
        .definition(DefinitionKind::Trait, "Default", default.definitions())
        .unwrap();
    let mut inventory = BTreeSet::new();
    for provider in &modules {
        let module = provider.records();
        let definitions = provider.definitions();
        for (index, implementation) in module.implementations.iter().enumerate() {
            let Some(interface) = &implementation.trait_type else {
                continue;
            };
            if interface.declaration != default_id {
                continue;
            }
            assert!(interface.arguments.is_empty());
            assert!(interface.associated_types.is_empty());
            assert!(implementation.bounds.is_empty());
            let name = match &implementation.for_type {
                Ty::Builtin(kind) => {
                    assert!(implementation.generic_params.is_empty());
                    format!("{kind:?}")
                }
                Ty::NativeObject(nominal) | Ty::Enum(nominal) => {
                    let [parameter] = implementation.generic_params.as_slice() else {
                        panic!("container defaults require exactly one unbounded parameter");
                    };
                    assert_eq!(nominal.arguments, [parameter.as_type()]);
                    definitions
                        .resolve(nominal.declaration)
                        .unwrap()
                        .last()
                        .unwrap()
                        .name
                        .to_owned()
                }
                receiver => panic!("unexpected standard Default target {receiver:?}"),
            };
            let expected_owner = match name.as_str() {
                "String" | "Vec" | "Option" => namespaces::type_owner(&name),
                _ => namespaces::module("core", "num"),
            };
            assert_eq!(module.identity, expected_owner);
            assert!(
                inventory.insert(name.clone()),
                "duplicate Default for {name}"
            );
            let [method] = implementation.methods.as_slice() else {
                panic!("Default implementations must bind exactly one method");
            };
            assert_eq!(method.name, "default");
            assert!(method.params.is_empty());
            assert!(method.bounds.is_empty());
            assert_eq!(method.generic_params, implementation.generic_params);
            assert_eq!(method.return_type, implementation.for_type);
            let CallableImplementation::Native(binding) = method.implementation else {
                panic!("standard defaults require explicit native bindings");
            };
            assert_eq!(
                definitions.resolve(binding).unwrap().module(),
                &expected_owner
            );
            let method_id = ModuleDecl::method_id(
                module.implementation_id(index, definitions).unwrap(),
                "default",
                definitions,
            )
            .unwrap();
            assert_eq!(
                binding, method_id,
                "each declaration owns its checked target"
            );
            assert!(!module.documentation[&method_id].is_empty());
        }
    }
    assert_eq!(
        inventory,
        [
            "Unit", "Bool", "I8", "I16", "I32", "I64", "U8", "U16", "U32", "U64", "F32", "F64",
            "String", "Vec", "Option",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect()
    );
}

#[test]
fn canonical_foundation_contracts_survive_scoped_publication_and_installation() {
    let first = catalog::declarations().unwrap();
    let second = catalog::declarations().unwrap();
    assert_eq!(first.len(), 36);
    let array_facade = first
        .iter()
        .find(|module| module.records().identity == namespaces::module("std", "array"))
        .expect("the empty std::array facade remains published")
        .records();
    assert!(array_facade.exports.is_empty());
    assert!(
        array_facade.dependencies.is_empty(),
        "an export-free facade must not acquire its canonical owner's provider closure"
    );
    assert_eq!(
        first
            .iter()
            .map(|module| module.records().types.len())
            .sum::<usize>(),
        33
    );
    assert_eq!(
        first
            .iter()
            .map(|module| module.records().traits.len())
            .sum::<usize>(),
        41
    );
    assert_eq!(
        first
            .iter()
            .map(|module| module.records().implementations.len())
            .sum::<usize>(),
        226
    );
    assert_eq!(
        first
            .iter()
            .map(|module| module.records().exports.len())
            .sum::<usize>(),
        93
    );
    assert_eq!(
        first
            .iter()
            .map(|module| module
                .records()
                .callable_requirements
                .values()
                .filter(|requirements| !requirements.is_empty())
                .count())
            .sum::<usize>(),
        // SR01 had 92 entries, five empty; fluent inherent methods add fourteen
        // empty entries, not new selected-call obligations.
        87
    );
    assert_eq!(
        first
            .iter()
            .map(|module| module.records().concrete_results.len())
            .sum::<usize>(),
        11
    );
    for module in &first {
        let owner = &module.records().identity;
        assert_ne!(owner.path, ["__foundation"]);
        if owner.package.0 == "std" {
            continue;
        }
        let facade = first
            .iter()
            .find(|candidate| {
                candidate.records().identity == namespaces::module("std", &owner.path.join("::"))
            })
            .unwrap();
        let public = module
            .records()
            .types
            .iter()
            .map(|ty| (&ty.name, ty.kind.definition_kind()))
            .chain(
                module
                    .records()
                    .traits
                    .iter()
                    .map(|contract| (&contract.name, DefinitionKind::Trait)),
            );
        for (name, kind) in public {
            let id = module
                .records()
                .definition(kind, name, module.definitions())
                .unwrap();
            assert_eq!(
                facade.records().exports[&ExportName::new(NameNamespace::Type, name)],
                id
            );
        }
    }
    assert_eq!(first[0].definitions().id(), second[0].definitions().id());
    let comparison = DefinitionContext::new().unwrap();
    let cancel = CancellationToken::default();
    assert_eq!(first.len(), second.len());
    for (first, second) in first.iter().zip(&second) {
        let left = first.import_into(&comparison, &cancel).unwrap();
        let right = second.import_into(&comparison, &cancel).unwrap();
        assert_ne!(left.definitions().id(), first.definitions().id());
        let (left, right) = (left.records(), right.records());
        assert_eq!(left.identity, right.identity);
        assert_eq!(left.types, right.types);
        assert_eq!(left.traits, right.traits);
        assert_eq!(left.implementations, right.implementations);
        assert_eq!(left.exports, right.exports);
        assert_eq!(
            left.native_declarations(&comparison.snapshot()).unwrap(),
            right.native_declarations(&comparison.snapshot()).unwrap()
        );
    }
    let default = first
        .iter()
        .find(|module| module.records().identity == namespaces::module("core", "default"))
        .unwrap();
    let owner = default.records();
    let id = owner
        .definition(DefinitionKind::Trait, "Default", default.definitions())
        .unwrap();
    let [contract] = owner.traits.as_slice() else {
        panic!("core::default must own exactly the Default declaration");
    };
    assert_eq!(contract.name, "Default");
    assert!(contract.conversion_adapter.is_none());
    assert!(contract.supertraits.is_empty());
    assert!(contract.generic_params.is_empty());
    assert!(contract.bounds.is_empty());
    assert!(contract.associated_consts.is_empty());
    assert!(contract.associated_types.is_empty());
    let [method] = contract.methods.as_slice() else {
        panic!("Default must have exactly one required static method");
    };
    assert_eq!(method.name, "default");
    assert_eq!(method.implementation, CallableImplementation::Required);
    assert!(method.params.is_empty());
    assert!(method.generic_params.is_empty());
    assert!(method.bounds.is_empty());
    assert_eq!(method.return_type, Ty::SelfType(id));
    assert!(!namespaces::is_language_module(&owner.identity));
    assert_eq!(
        first
            .iter()
            .flat_map(|module| &module.records().traits)
            .filter(|contract| contract.name == "Default")
            .count(),
        1
    );
    for facade in [namespaces::module("std", "default"), namespaces::prelude()] {
        let facade = first
            .iter()
            .find(|module| module.records().identity == facade)
            .unwrap();
        assert_eq!(
            facade.records().exports[&ExportName::new(NameNamespace::Type, "Default")],
            id
        );
        assert!(facade.records().dependencies.contains(&owner.identity));
    }
    let modules = crate::modules().unwrap();
    let mut first = Runtime::default();
    let mut second = Runtime::default();
    NativeModule::install_all(&modules, &mut first).unwrap();
    NativeModule::install_all(&modules, &mut second).unwrap();
    assert!(NativeModule::install_all(&modules, &mut first).is_err());
}

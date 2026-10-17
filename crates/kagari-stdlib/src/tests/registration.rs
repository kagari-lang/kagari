//! Scoped foundation declarations and storage admission require no source compiler.
use crate::{catalog, declarations::StandardDeclarations, namespaces};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionKind, map::DefinitionContext},
};
use kagari_runtime::{
    Runtime,
    native::{
        builder::ModuleBuilder,
        module::NativeModule,
        storage::{NativePayload, NativeStorage},
    },
    value::Value,
};
use kagari_types::{
    callable::CallableImplementation,
    declaration::names::{ExportName, NameNamespace},
    ty::Ty,
};

#[derive(Debug)]
struct EmptyPayload;

impl NativePayload for EmptyPayload {
    fn trace<'payload>(&'payload self, _visit: &mut dyn FnMut(&'payload Value)) {}

    fn units(&self) -> usize {
        0
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
fn canonical_foundation_contracts_survive_scoped_publication_and_installation() {
    let first = catalog::declarations().unwrap();
    let second = catalog::declarations().unwrap();
    assert_ne!(first[0].definitions().id(), second[0].definitions().id());
    let comparison = DefinitionContext::new().unwrap();
    let cancel = CancellationToken::default();
    assert_eq!(first.len(), second.len());
    for (first, second) in first.iter().zip(&second) {
        let left = first.import_into(&comparison, &cancel).unwrap();
        let right = second.import_into(&comparison, &cancel).unwrap();
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

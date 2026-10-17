//! Scoped foundation declarations and storage admission require no source compiler.
use crate::{catalog, declarations::StandardDeclarations};
use kagari_common::{cancellation::CancellationToken, identity::map::DefinitionContext};
use kagari_runtime::{
    Runtime,
    native::{
        builder::ModuleBuilder,
        module::NativeModule,
        storage::{NativePayload, NativeStorage},
    },
    value::Value,
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
    let modules = crate::modules().unwrap();
    let mut first = Runtime::default();
    let mut second = Runtime::default();
    NativeModule::install_all(&modules, &mut first).unwrap();
    NativeModule::install_all(&modules, &mut second).unwrap();
    assert!(NativeModule::install_all(&modules, &mut first).is_err());
}

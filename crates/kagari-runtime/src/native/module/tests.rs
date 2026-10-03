use crate::{
    Runtime,
    native::{
        binding::{Codec, NativeBinding},
        builder::ModuleBuilder,
        catalog::DeclarationCatalog,
        declarations::{FunctionDecl, MethodDecl},
        foundation,
        language::LanguageContracts,
        module::NativeModule,
        types::Type,
    },
    value::Value,
};

fn assert_catalog_eq(left: &DeclarationCatalog, right: &DeclarationCatalog) {
    assert_eq!(left.types, right.types);
    assert_eq!(left.traits, right.traits);
    assert_eq!(left.declarations, right.declarations);
    assert_eq!(left.implementations, right.implementations);
}

#[test]
fn reused_closures_preserve_exact_foundation_binding_requirements() {
    let module = foundation::module().unwrap();
    for registration in module.bindings.iter() {
        let fresh = module
            .catalog
            .dependency_closure(
                module.owned.traits.keys(),
                &registration.declarations,
                [module.declaration.as_ref()],
            )
            .unwrap();
        assert_catalog_eq(&registration.required_catalog, &fresh.catalog);
    }
}

#[test]
fn binding_specific_foreign_dependencies_do_not_leak_to_other_bindings() {
    let language = LanguageContracts::default();
    let mut provider = ModuleBuilder::new("test::provider", &language);
    let mut remote = provider.define_trait("Remote");
    remote.define_method(MethodDecl::instance("read")).unwrap();
    let remote = remote.finish().unwrap();
    let provider = provider.finish().unwrap();
    let mut consumer = ModuleBuilder::new("test::consumer", &language)
        .with_modules(&[&provider])
        .unwrap();
    let inspect = consumer
        .define_function(FunctionDecl::new("inspect").parameter("value", remote.apply([]).ty()))
        .unwrap();
    consumer
        .bind_with(
            inspect,
            NativeBinding::new(vec![Codec::Value], Codec::Value, |_| Ok(Value::Unit)),
        )
        .unwrap();
    let local = consumer
        .define_function(FunctionDecl::new("local"))
        .unwrap();
    consumer
        .bind_with(
            local,
            NativeBinding::new(vec![], Codec::Value, |_| Ok(Value::Unit)),
        )
        .unwrap();
    let module = consumer.finish().unwrap();
    for registration in module.bindings.iter() {
        let declaration = &registration.declarations[0];
        let name = declaration.declaration.path.last().unwrap().name.as_str();
        assert_eq!(
            registration.required_catalog.traits.len(),
            usize::from(name == "inspect")
        );
        let fresh = module
            .catalog
            .dependency_closure(
                module.owned.traits.keys(),
                &registration.declarations,
                [module.declaration.as_ref()],
            )
            .unwrap();
        assert_catalog_eq(&registration.required_catalog, &fresh.catalog);
    }
    let mut runtime = Runtime::default();
    let before = runtime.native_entries.catalog.clone();
    assert!(module.install(&mut runtime).is_err());
    assert_catalog_eq(&runtime.native_entries.catalog, &before);
    provider.install(&mut runtime).unwrap();
    module.install(&mut runtime).unwrap();
}

fn application(names: &[&str]) -> NativeModule {
    let mut module = ModuleBuilder::new("test::application", &LanguageContracts::default());
    for name in names {
        let function = module
            .define_function(FunctionDecl::new(*name).returns(Type::i32()))
            .unwrap();
        module
            .bind_with(
                function,
                NativeBinding::new(vec![], Codec::Scalar(Type::i32().abi().clone()), |_| {
                    Ok(Value::I32(42))
                }),
            )
            .unwrap();
    }
    module.finish().unwrap()
}

#[test]
fn shared_registrations_keep_installation_and_failure_state_runtime_local() {
    let module = application(&["value"]);
    let mut first = Runtime::default();
    let mut second = Runtime::default();
    module.install(&mut first).unwrap();
    let before = first.native_entries.catalog.clone();
    assert!(
        application(&["added", "value"])
            .install(&mut first)
            .is_err()
    );
    assert_catalog_eq(&first.native_entries.catalog, &before);
    // A failed batch must not retain its earlier successful entry.
    application(&["added"]).install(&mut first).unwrap();
    module.install(&mut second).unwrap();
    assert!(module.install(&mut second).is_err());
    assert!(
        !second.native_entries.catalog.declarations.keys().any(|id| {
            id.path
                .last()
                .is_some_and(|segment| segment.name == "added")
        })
    );
    assert_eq!(first.gc().active_roots(), 0);
    assert_eq!(second.gc().active_roots(), 0);
}

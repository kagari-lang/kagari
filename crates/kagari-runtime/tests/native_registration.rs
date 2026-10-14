//! Registration contracts need no source compiler or standard-library fixtures.
use kagari_runtime::{
    Runtime,
    native::{
        binding::{Codec, NativeBinding, NativeResult},
        builder::ModuleBuilder,
        catalog::DeclarationCatalog,
        context::CallContext,
        declarations::{CallableRequirement, FunctionDecl, MethodDecl},
        module::NativeModule,
        storage::{NativePayload, NativeStorage},
        types::{TraitRef, Type},
        views::{SequenceHandle, SequenceMutHandle},
    },
    value::Value,
};
use kagari_types::{declaration::module::ModuleDecl, scalar::BuiltinType};

fn count(_cx: &mut CallContext<'_>) -> NativeResult<u64> {
    Ok(1)
}

fn sequence_len(_cx: &mut CallContext<'_>, values: SequenceHandle<'_>) -> NativeResult<usize> {
    values.len()
}

#[derive(Debug)]
struct EmptyPayload;

impl NativePayload for EmptyPayload {
    fn trace<'payload>(&'payload self, _visit: &mut dyn FnMut(&'payload Value)) {}

    fn units(&self) -> usize {
        0
    }
}

#[test]
fn same_physical_integer_layout_does_not_erase_usize() {
    let mut module =
        ModuleBuilder::new("example::mismatch", &DeclarationCatalog::default()).unwrap();
    let function = module
        .define_function(FunctionDecl::new("count").returns(Type::usize()))
        .unwrap();
    let error = module.bind(function, count).unwrap_err();
    assert!(error.message().contains("codec"));
}

#[test]
fn explicit_binding_uses_the_same_signature_check() {
    let mut module =
        ModuleBuilder::new("example::explicit", &DeclarationCatalog::default()).unwrap();
    let function = module
        .define_function(FunctionDecl::new("count").returns(Type::usize()))
        .unwrap();
    let wrong = NativeBinding::new(vec![], Codec::Exact(Type::u64()), |_| Ok(Value::U64(0)));
    assert!(module.bind(function.clone(), wrong).is_err());
    let right = NativeBinding::new(vec![], Codec::Exact(Type::usize()), |_| Ok(Value::U64(0)));
    module.bind(function, right).unwrap();
    module.finish().unwrap();
}

#[test]
fn finalization_rejects_a_declaration_without_an_entry() {
    let mut module =
        ModuleBuilder::new("example::missing", &DeclarationCatalog::default()).unwrap();
    module
        .define_function(FunctionDecl::new("missing"))
        .unwrap();
    assert!(module.finish().unwrap_err().message().contains("missing"));
}

#[test]
fn sequence_conversion_rejects_opaque_storage_at_registration() {
    let mut module = ModuleBuilder::new("example::opaque", &DeclarationCatalog::default()).unwrap();
    let mut declaration = module.define_type("Opaque").unwrap();
    declaration
        .native_storage(NativeStorage::payload::<EmptyPayload>())
        .unwrap();
    let opaque = declaration.finish().unwrap();
    let len = module
        .define_function(
            FunctionDecl::new("len")
                .parameter("values", opaque.apply([]).unwrap())
                .returns(Type::usize()),
        )
        .unwrap();
    assert!(
        module
            .bind(len.clone(), sequence_len)
            .unwrap_err()
            .message()
            .contains("codec")
    );
    module
        .bind(
            len,
            NativeBinding::new(vec![opaque.codec()], Codec::Exact(Type::usize()), |_| {
                Ok(Value::U64(0))
            }),
        )
        .unwrap();
    module.finish().unwrap();
}

#[test]
fn sequence_receiver_groups_allow_read_and_write_member_views() {
    let mut module = ModuleBuilder::new("example::views", &DeclarationCatalog::default()).unwrap();
    let mut declaration = module.define_type("Buffer").unwrap();
    let item = declaration.type_parameter("T").unwrap();
    declaration.sequence_storage(&item).unwrap();
    let buffer = declaration.finish().unwrap();
    let mut read = module.define_trait("Read").unwrap();
    read.define_method(MethodDecl::instance("len").returns(Type::usize()))
        .unwrap();
    let read = read.finish().unwrap();
    let mut write = module.define_trait("Write").unwrap();
    write
        .define_method(MethodDecl::instance("reverse"))
        .unwrap();
    let write = write.finish().unwrap();
    module
        .implement(buffer.apply([Type::i32()]).unwrap(), |group| {
            group.receiver_codec(Codec::Sequence)?;
            group.trait_impl(read.apply([])?, |methods| methods.bind("len", sequence_len))?;
            group.trait_impl(write.apply([])?, |methods| {
                methods.bind(
                    "reverse",
                    |_cx: &mut CallContext<'_>,
                     mut values: SequenceMutHandle<'_>|
                     -> NativeResult<()> {
                        values.with_slice_mut::<i32, _>(|values| {
                            values.reverse();
                            Ok(())
                        })
                    },
                )
            })
        })
        .unwrap();
    module
        .finish()
        .unwrap()
        .install(&mut Runtime::default())
        .unwrap();
}

#[test]
fn missing_or_unknown_trait_member_is_rejected() {
    let mut module =
        ModuleBuilder::new("example::missing_trait", &DeclarationCatalog::default()).unwrap();
    let mut declaration = module.define_trait("Measure").unwrap();
    declaration
        .define_method(MethodDecl::instance("measure").returns(Type::usize()))
        .unwrap();
    let measure = declaration.finish().unwrap();
    let error = module
        .implement(Type::i32(), |implementation| {
            implementation.trait_impl(measure.apply([])?, |_| Ok(()))
        })
        .unwrap_err();
    assert!(error.message().contains("missing method"));
}

#[test]
fn generic_receiver_groups_emit_independent_impl_binders() {
    let mut module =
        ModuleBuilder::new("example::generic", &DeclarationCatalog::default()).unwrap();
    let mut declaration = module.define_type("Buffer").unwrap();
    declaration.type_parameter("T").unwrap();
    declaration
        .native_storage(NativeStorage::new(|_| Ok(EmptyPayload)))
        .unwrap();
    let buffer = declaration.finish().unwrap();
    let mut declaration = module.define_trait("Read").unwrap();
    let item = declaration.type_parameter("Item").unwrap();
    declaration
        .define_method(MethodDecl::instance("read").returns(item.ty()))
        .unwrap();
    let read = declaration.finish().unwrap();
    let mut declaration = module.define_trait("Write").unwrap();
    let item = declaration.type_parameter("Item").unwrap();
    declaration
        .define_method(MethodDecl::instance("write").parameter("value", item.ty()))
        .unwrap();
    let write = declaration.finish().unwrap();
    module
        .implement(buffer.clone(), |group| {
            let item = group.parameter("T")?.ty();
            group.receiver_codec(buffer.codec())?;
            group.trait_impl(read.apply([item.clone()])?, |methods| {
                methods.bind(
                    "read",
                    NativeBinding::new(vec![buffer.codec()], Codec::Value, |_| Ok(Value::I32(1))),
                )
            })?;
            group.trait_impl(write.apply([item])?, |methods| {
                methods.bind(
                    "write",
                    NativeBinding::new(
                        vec![buffer.codec(), Codec::Value],
                        Codec::Exact(Type::unit()),
                        |_| Ok(Value::Unit),
                    ),
                )
            })
        })
        .unwrap();
    let module = module.finish().unwrap();
    let declarations = module.declaration();
    let first = &declarations.implementations[0];
    let second = &declarations.implementations[1];
    assert_ne!(
        first.generic_params[0].owner,
        second.generic_params[0].owner
    );
    assert_eq!(
        first.methods[0].return_type,
        first.generic_params[0].as_type()
    );
    assert_eq!(
        second.methods[0].params[1].ty,
        second.generic_params[0].as_type()
    );
    module.install(&mut Runtime::default()).unwrap();
}

#[test]
fn full_markdown_survives_builder_completion_and_cross_module_inheritance() {
    let markdown = "Read a value.\n\n# Examples\n\n```kgr\nfn answer() -> i32 { 42 }\n```\n\n- Preserve **Markdown**.";
    let mut provider =
        ModuleBuilder::new("docs::provider", &DeclarationCatalog::default()).unwrap();
    provider.documentation("# Provider\n\nFull module overview.");
    let mut contract = provider.define_trait("Read").unwrap();
    contract.documentation("# Read\n\nTrait overview.");
    contract.associated_type("Item", []).unwrap();
    assert!(
        contract
            .associated_type_documentation("Missing", "invalid")
            .is_err()
    );
    contract
        .associated_type_documentation("Item", "The returned item.")
        .unwrap();
    let read = contract
        .define_method(
            MethodDecl::instance("read")
                .returns(Type::i32())
                .documentation(markdown),
        )
        .unwrap();
    let overridden = contract
        .define_method(
            MethodDecl::instance("other")
                .returns(Type::i32())
                .documentation(markdown),
        )
        .unwrap();
    let reference = contract.finish().unwrap();
    let provider = provider.finish().unwrap();
    let metadata = provider.to_declaration();
    let declared = metadata.records();
    assert_eq!(declared.documentation[&read.id()], markdown);
    assert_eq!(declared.documentation[&overridden.id()], markdown);
    assert_eq!(
        declared.module_documentation,
        "# Provider\n\nFull module overview."
    );

    let mut consumer = ModuleBuilder::new("docs::consumer", &provider.catalog()).unwrap();
    let mut ty = consumer.define_type("Value").unwrap();
    ty.documentation("# Value\n\nShared payload.");
    ty.native_storage(NativeStorage::new(|_| Ok(EmptyPayload)))
        .unwrap();
    let value = ty.finish().unwrap();
    consumer
        .implement(value, |group| {
            group.trait_impl(reference.apply([])?, |methods| {
                methods.associated_type("Item", Type::i32())?;
                methods.documentation("other", "Implementation-specific **override**.")?;
                assert!(methods.documentation("missing", "invalid").is_err());
                for method in ["read", "other"] {
                    methods.bind(
                        method,
                        NativeBinding::new(vec![Codec::Value], Codec::Value, |_| {
                            Ok(Value::I32(42))
                        }),
                    )?;
                }
                Ok(())
            })?;
            group.inherent_impl(|methods| {
                let method = methods.define_method(
                    MethodDecl::instance("own")
                        .returns(Type::i32())
                        .documentation(markdown),
                )?;
                methods.bind(
                    method,
                    NativeBinding::new(vec![Codec::Value], Codec::Value, |_| Ok(Value::I32(42))),
                )
            })
        })
        .unwrap();
    let metadata = consumer.finish().unwrap().to_declaration();
    let declared = metadata.records();
    let definitions = metadata.definitions();
    assert!(
        declared
            .documentation
            .values()
            .any(|text| text == "# Value\n\nShared payload.")
    );
    assert_eq!(
        declared.documentation[&ModuleDecl::method_id(
            declared.implementation_id(0, definitions).unwrap(),
            "read",
            definitions
        )
        .unwrap()],
        markdown
    );
    assert_eq!(
        declared.documentation[&ModuleDecl::method_id(
            declared.implementation_id(0, definitions).unwrap(),
            "other",
            definitions
        )
        .unwrap()],
        "Implementation-specific **override**."
    );
    assert_eq!(
        declared.documentation[&ModuleDecl::method_id(
            declared.implementation_id(1, definitions).unwrap(),
            "own",
            definitions
        )
        .unwrap()],
        markdown
    );
}

#[test]
fn scoped_type_inputs_preserve_snapshots_and_compare_complete_identity() {
    let mut first = ModuleBuilder::new("scope::one", &DeclarationCatalog::default()).unwrap();
    let first_type = first.define_enum("Item").unwrap().finish().unwrap();
    let retained = first_type.definitions().clone();
    let later = first.define_enum("Later").unwrap().finish().unwrap();
    assert!(Type::from_semantic(later.apply([]).unwrap().abi().clone(), &retained).is_err());

    let mut equivalent = ModuleBuilder::new("scope::one", &DeclarationCatalog::default()).unwrap();
    let equivalent_type = equivalent.define_enum("Item").unwrap().finish().unwrap();
    assert!(
        Type::from_semantic(
            first_type.apply([]).unwrap().abi().clone(),
            equivalent_type.definitions()
        )
        .is_err()
    );

    let mut foreign = ModuleBuilder::new("scope::other", &DeclarationCatalog::default()).unwrap();
    let foreign_type = foreign.define_enum("Item").unwrap().finish().unwrap();
    let first = first.finish().unwrap();
    let foreign = foreign.finish().unwrap();
    let providers = DeclarationCatalog::from_modules(&[&first, &foreign]).unwrap();
    let mut consumer = ModuleBuilder::new("scope::consumer", &providers).unwrap();
    let pair = Type::tuple([
        first_type.apply([]).unwrap(),
        equivalent_type.apply([]).unwrap(),
    ])
    .unwrap();
    let exact =
        Type::tuple([first_type.apply([]).unwrap(), first_type.apply([]).unwrap()]).unwrap();
    let different = Type::tuple([
        first_type.apply([]).unwrap(),
        foreign_type.apply([]).unwrap(),
    ])
    .unwrap();
    let function = consumer
        .define_function(FunctionDecl::new("consume").parameter("pair", pair))
        .unwrap();
    assert!(
        consumer
            .bind(
                function.clone(),
                NativeBinding::new(
                    vec![Codec::Exact(different)],
                    Codec::Exact(Type::unit()),
                    |_| Ok(Value::Unit)
                )
            )
            .is_err()
    );
    consumer
        .bind(
            function,
            NativeBinding::new(
                vec![Codec::Exact(exact)],
                Codec::Exact(Type::unit()),
                |_| Ok(Value::Unit),
            ),
        )
        .unwrap();
    let consumer = consumer.finish().unwrap();
    let mut runtime = Runtime::default();
    first.install(&mut runtime).unwrap();
    consumer.install(&mut runtime).unwrap();
}

// The selected-call contract needs a provider trait, not a source compiler or the
// full standard library. Its ordinary required method supplies no implementation.
fn hash_provider() -> (NativeModule, TraitRef) {
    let mut provider =
        ModuleBuilder::new("contracts::hash", &DeclarationCatalog::default()).unwrap();
    let mut hash = provider.define_trait("Hash").unwrap();
    hash.define_method(MethodDecl::instance("hash").returns(Type::u64()))
        .unwrap();
    let hash = hash.finish().unwrap();
    (provider.finish().unwrap(), hash)
}

#[test]
fn selected_callbacks_require_proven_declared_bounds() {
    let (provider, hash) = hash_provider();
    for declare_bound in [false, true] {
        let mut module =
            ModuleBuilder::new("example::selected_bounds", &provider.catalog()).unwrap();
        let function = module
            .define_function(FunctionDecl::new("inspect"))
            .unwrap();
        module
            .function(&function, |function| {
                let item = function.type_parameter("T")?.ty();
                function.parameter("value", item.clone())?;
                if declare_bound {
                    function.bound(item.clone(), hash.apply([])?)?;
                }
                function.requires(CallableRequirement::method(item, hash.method("hash")?)?)?;
                Ok(())
            })
            .unwrap();
        module
            .bind(
                function,
                NativeBinding::new(vec![Codec::Value], Codec::Value, |_| Ok(Value::Unit)),
            )
            .unwrap();
        assert_eq!(module.finish().is_ok(), declare_bound);
    }
}

#[test]
fn generic_default_is_derived_from_the_method_contract() {
    let (provider, hash) = hash_provider();
    let mut module = ModuleBuilder::new("example::defaults", &provider.catalog()).unwrap();
    let mut declaration = module.define_trait("Echo").unwrap();
    let item = declaration.type_parameter("T").unwrap().ty();
    let method = declaration
        .define_method(MethodDecl::instance("echo"))
        .unwrap();
    declaration
        .method(&method, |method| {
            let key = method.type_parameter("K")?.ty();
            method.parameter("item", item)?;
            method.parameter("key", key.clone())?;
            method.returns(key.clone())?;
            method.bound(key.clone(), hash.apply([])?)?;
            method.requires(CallableRequirement::method(key, hash.method("hash")?)?)?;
            Ok(())
        })
        .unwrap();
    declaration
        .bind(
            method,
            NativeBinding::new(
                vec![Codec::Value, Codec::Value, Codec::Value],
                Codec::Value,
                |cx| cx.argument(2),
            ),
        )
        .unwrap();
    let echo = declaration.finish().unwrap();
    module
        .implement(Type::i32(), |group| {
            group.trait_impl(echo.apply([Type::scalar(BuiltinType::String)])?, |_| Ok(()))
        })
        .unwrap();
    let module = module.finish().unwrap();
    let declarations = module.declaration();
    let template = &declarations.functions[0];
    assert_eq!(template.generic_params.len(), 3);
    assert_eq!(template.params[0].ty, template.generic_params[0].as_type());
    assert_eq!(template.params[1].ty, template.generic_params[1].as_type());
    assert_eq!(template.return_type, template.generic_params[2].as_type());
    let mut runtime = Runtime::default();
    provider.install(&mut runtime).unwrap();
    module.install(&mut runtime).unwrap();
}

#[test]
fn native_default_rejects_unproven_operations_and_incompatible_codecs() {
    let (provider, hash) = hash_provider();
    for declare_bound in [false, true] {
        let mut module =
            ModuleBuilder::new("example::default_bounds", &provider.catalog()).unwrap();
        let mut declaration = module.define_trait("Inspect").unwrap();
        let method = declaration
            .define_method(MethodDecl::instance("inspect"))
            .unwrap();
        declaration
            .method(&method, |method| {
                let key = method.type_parameter("K")?.ty();
                method.parameter("key", key.clone())?;
                if declare_bound {
                    method.bound(key.clone(), hash.apply([])?)?;
                }
                method.requires(CallableRequirement::method(key, hash.method("hash")?)?)?;
                Ok(())
            })
            .unwrap();
        declaration
            .bind(
                method.clone(),
                NativeBinding::new(vec![Codec::Value, Codec::Value], Codec::Value, |_| {
                    Ok(Value::Unit)
                }),
            )
            .unwrap();
        assert!(
            declaration
                .bind(
                    method,
                    NativeBinding::new(vec![Codec::Value, Codec::Value], Codec::Value, |_| Ok(
                        Value::Unit
                    ),)
                )
                .is_err()
        );
        declaration.finish().unwrap();
        assert_eq!(module.finish().is_ok(), declare_bound);
    }
    let mut module = ModuleBuilder::new("example::default_codec", &provider.catalog()).unwrap();
    let mut declaration = module.define_trait("Count").unwrap();
    let method = declaration
        .define_method(MethodDecl::instance("count").returns(Type::usize()))
        .unwrap();
    declaration
        .bind(
            method,
            NativeBinding::new(vec![Codec::Value], Codec::Exact(Type::i32()), |_| {
                Ok(Value::I32(0))
            }),
        )
        .unwrap();
    assert!(
        declaration
            .finish()
            .unwrap_err()
            .message()
            .contains("codec")
    );
}

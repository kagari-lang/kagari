use kagari_hir::native::render::declaration_source;
use kagari_runtime::{
    Runtime,
    native::{
        binding::{Codec, NativeBinding, NativeResult},
        builder::ModuleBuilder,
        catalog::DeclarationCatalog,
        context::CallContext,
        declarations::{CallableRequirement, FunctionDecl, MethodDecl},
        storage::{NativePayload, NativeStorage},
        types::Type,
        views::{SequenceHandle, SequenceMutHandle},
    },
    value::Value,
};
use kagari_stdlib::{
    catalog as foundation_catalog, declarations::StandardDeclarations, namespaces,
};
use kagari_types::{declaration::module::ModuleDecl, scalar::BuiltinType};

fn add(_cx: &mut CallContext<'_>, left: i32, right: i32) -> NativeResult<i32> {
    Ok(left + right)
}

fn count(_cx: &mut CallContext<'_>) -> NativeResult<u64> {
    Ok(1)
}

#[test]
fn ordinary_rust_binding_keeps_explicit_kagari_signature() {
    let language = StandardDeclarations::default();
    let mut module = ModuleBuilder::new(
        "example::math",
        &language.catalog().expect("explicit standard providers"),
    );
    let function = module
        .define_function(
            FunctionDecl::new("add")
                .parameter("left", Type::i32())
                .parameter("right", Type::i32())
                .returns(Type::i32()),
        )
        .unwrap();
    module.bind(function, add).unwrap();
    let module = module.finish().unwrap();
    module.install(&mut Runtime::default()).unwrap();
    let text = declaration_source(
        &module.to_declaration().unwrap(),
        &foundation_catalog::shared(),
    )
    .unwrap()
    .text;
    assert!(
        text.contains("fn add(left: i32, right: i32) -> i32"),
        "{text}"
    );
}

#[test]
fn same_physical_integer_layout_does_not_erase_usize() {
    let mut module = ModuleBuilder::new(
        "example::mismatch",
        &StandardDeclarations::default()
            .catalog()
            .expect("explicit standard providers"),
    );
    let function = module
        .define_function(FunctionDecl::new("count").returns(Type::usize()))
        .unwrap();
    let error = module.bind(function, count).unwrap_err();
    assert!(error.message().contains("codec"));
}

#[test]
fn explicit_binding_uses_the_same_signature_check() {
    let mut module = ModuleBuilder::new(
        "example::explicit",
        &StandardDeclarations::default()
            .catalog()
            .expect("explicit standard providers"),
    );
    let function = module
        .define_function(FunctionDecl::new("count").returns(Type::usize()))
        .unwrap();
    let wrong = NativeBinding::new(vec![], Codec::Scalar(Type::u64().abi().clone()), |_| {
        Ok(Value::U64(0))
    });
    assert!(module.bind_with(function.clone(), wrong).is_err());
    let right = NativeBinding::new(vec![], Codec::Scalar(Type::usize().abi().clone()), |_| {
        Ok(Value::U64(0))
    });
    module.bind_with(function, right).unwrap();
    module.finish().unwrap();
}

#[test]
fn finalization_rejects_a_declaration_without_an_entry() {
    let mut module = ModuleBuilder::new(
        "example::missing",
        &StandardDeclarations::default()
            .catalog()
            .expect("explicit standard providers"),
    );
    module
        .define_function(FunctionDecl::new("missing"))
        .unwrap();
    assert!(module.finish().unwrap_err().message().contains("missing"));
}

#[test]
fn builtin_inherent_methods_require_the_language_owner_even_for_raw_declarations() {
    use kagari_common::identity::ModuleIdentity;
    use kagari_types::{
        declaration::module::{ImplDecl, ModuleDecl},
        ty::Ty,
    };
    let string = Ty::Builtin(BuiltinType::String);
    let mut owner = ModuleDecl::new(namespaces::type_owner("String"));
    assert!(owner.owns_inherent_receiver(&string, &namespaces::receiver_owner));
    owner.implementations.push(ImplDecl {
        generic_params: vec![],
        bounds: vec![],
        trait_type: None,
        for_type: string.clone(),
        methods: vec![],
    });
    owner.validate(&namespaces::receiver_owner).unwrap();
    owner.identity = ModuleIdentity::single_file("foreign.kgr");
    assert!(!owner.owns_inherent_receiver(&string, &namespaces::receiver_owner));
    assert!(owner.validate(&namespaces::receiver_owner).is_err());

    let mut foreign = ModuleBuilder::new(
        "example::foreign",
        &StandardDeclarations::default()
            .catalog()
            .expect("explicit standard providers"),
    );
    let error = foreign
        .implement(Type::scalar(BuiltinType::String), |group| {
            group.inherent_impl(|_| Ok(()))
        })
        .unwrap_err();
    assert!(error.message().contains("defining module"));
}

fn sequence_len(_cx: &mut CallContext<'_>, values: SequenceHandle<'_>) -> NativeResult<usize> {
    values.len()
}

#[test]
fn declaration_storage_binding_preserves_local_contract() {
    let language = StandardDeclarations::default();
    let catalog = language.catalog().unwrap();
    let future = catalog.future_type().unwrap();
    let declaration = language
        .declarations()
        .iter()
        .find(|declaration| declaration.identity == future.id().module)
        .unwrap()
        .as_ref()
        .clone();
    let builder =
        || ModuleBuilder::from_declaration(declaration.clone(), &catalog, Default::default());
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
    let mut foreign = ModuleBuilder::new("example::foreign_storage", &catalog);
    assert!(
        foreign
            .bind_storage(&future, NativeStorage::future())
            .is_err()
    );
    foreign.finish().unwrap();
}

#[test]
fn sequence_conversion_rejects_opaque_storage_at_registration() {
    let mut module = ModuleBuilder::new(
        "example::opaque",
        &StandardDeclarations::default()
            .catalog()
            .expect("explicit standard providers"),
    );
    let mut declaration = module.define_type("Opaque");
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
        .bind_with(
            len,
            NativeBinding::new(
                vec![opaque.codec()],
                Codec::Scalar(Type::usize().abi().clone()),
                |_| Ok(Value::U64(0)),
            ),
        )
        .unwrap();
    module.finish().unwrap();
}

#[test]
fn sequence_receiver_groups_allow_read_and_write_member_views() {
    let mut module = ModuleBuilder::new(
        "example::views",
        &StandardDeclarations::default()
            .catalog()
            .expect("explicit standard providers"),
    );
    let mut declaration = module.define_type("Buffer");
    let item = declaration.type_parameter("T").unwrap();
    declaration.sequence_storage(&item).unwrap();
    let buffer = declaration.finish().unwrap();
    let mut read = module.define_trait("Read");
    read.define_method(MethodDecl::instance("len").returns(Type::usize()))
        .unwrap();
    let read = read.finish().unwrap();
    let mut write = module.define_trait("Write");
    write
        .define_method(MethodDecl::instance("reverse"))
        .unwrap();
    let write = write.finish().unwrap();
    module
        .implement(buffer.apply([Type::i32()]).unwrap(), |group| {
            group.receiver_codec(Codec::Sequence)?;
            group.trait_impl(read.apply([]), |methods| methods.bind("len", sequence_len))?;
            group.trait_impl(write.apply([]), |methods| {
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
fn trait_groups_bind_declared_members_and_associated_outputs() {
    let language = StandardDeclarations::default();
    let mut module = ModuleBuilder::new(
        "example::traits",
        &language.catalog().expect("explicit standard providers"),
    );
    let mut trait_builder = module.define_trait("Measure");
    let output = trait_builder.associated_type("Output", []).unwrap();
    trait_builder
        .define_method(MethodDecl::instance("measure").returns(output))
        .unwrap();
    let measure = trait_builder.finish().unwrap();
    module
        .implement(Type::i32(), |implementation| {
            implementation.receiver_codec(Codec::Scalar(Type::i32().abi().clone()))?;
            implementation.trait_impl(measure.apply([]), |methods| {
                methods.associated_type("Output", Type::usize())?;
                methods.bind(
                    "measure",
                    |_cx: &mut CallContext<'_>, value: i32| -> NativeResult<usize> {
                        Ok(value as usize)
                    },
                )
            })
        })
        .unwrap();
    let module = module.finish().unwrap();
    let mut runtime = Runtime::default();
    module.install(&mut runtime).unwrap();
    assert!(module.install(&mut runtime).is_err());
    assert!(
        declaration_source(
            &module.to_declaration().unwrap(),
            &foundation_catalog::shared()
        )
        .unwrap()
        .text
        .contains("type Output = usize")
    );
}

#[test]
fn missing_or_unknown_trait_member_is_rejected() {
    let mut module = ModuleBuilder::new(
        "example::missing_trait",
        &StandardDeclarations::default()
            .catalog()
            .expect("explicit standard providers"),
    );
    let mut declaration = module.define_trait("Measure");
    declaration
        .define_method(MethodDecl::instance("measure").returns(Type::usize()))
        .unwrap();
    let measure = declaration.finish().unwrap();
    let error = module
        .implement(Type::i32(), |implementation| {
            implementation.trait_impl(measure.apply([]), |_| Ok(()))
        })
        .unwrap_err();
    assert!(error.message().contains("missing method"));
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
fn generic_receiver_groups_emit_independent_impl_binders() {
    let mut module = ModuleBuilder::new(
        "example::generic",
        &StandardDeclarations::default()
            .catalog()
            .expect("explicit standard providers"),
    );
    let mut declaration = module.define_type("Buffer");
    declaration.type_parameter("T").unwrap();
    declaration
        .native_storage(NativeStorage::new(|_| Ok(EmptyPayload)))
        .unwrap();
    let buffer = declaration.finish().unwrap();
    let mut declaration = module.define_trait("Read");
    let item = declaration.type_parameter("Item").unwrap();
    declaration
        .define_method(MethodDecl::instance("read").returns(item.ty()))
        .unwrap();
    let read = declaration.finish().unwrap();
    let mut declaration = module.define_trait("Write");
    let item = declaration.type_parameter("Item").unwrap();
    declaration
        .define_method(MethodDecl::instance("write").parameter("value", item.ty()))
        .unwrap();
    let write = declaration.finish().unwrap();
    module
        .implement(buffer.clone(), |group| {
            let item = group.parameter("T")?.ty();
            group.receiver_codec(buffer.codec())?;
            group.trait_impl(read.apply([item.clone()]), |methods| {
                methods.bind_with(
                    "read",
                    NativeBinding::new(vec![buffer.codec()], Codec::Value, |_| Ok(Value::I32(1))),
                )
            })?;
            group.trait_impl(write.apply([item]), |methods| {
                methods.bind_with(
                    "write",
                    NativeBinding::new(
                        vec![buffer.codec(), Codec::Value],
                        Codec::Scalar(Type::unit().abi().clone()),
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
fn selected_callbacks_require_proven_declared_bounds() {
    let language = StandardDeclarations::default();
    for declare_bound in [false, true] {
        let mut module = ModuleBuilder::new(
            "example::selected_bounds",
            &language.catalog().expect("explicit standard providers"),
        );
        let function = module
            .define_function(FunctionDecl::new("inspect"))
            .unwrap();
        module
            .function(&function, |function| {
                let item = function.type_parameter("T")?.ty();
                function.parameter("value", item.clone());
                if declare_bound {
                    function.bound(item.clone(), language.hash().apply([]));
                }
                function.requires(CallableRequirement::method(
                    item,
                    language.hash().method("hash")?,
                ));
                Ok(())
            })
            .unwrap();
        module
            .bind_with(
                function,
                NativeBinding::new(vec![Codec::Value], Codec::Value, |_| Ok(Value::Unit)),
            )
            .unwrap();
        assert_eq!(module.finish().is_ok(), declare_bound);
    }
}

#[test]
fn generic_default_is_derived_from_the_method_contract() {
    let language = StandardDeclarations::default();
    let mut module = ModuleBuilder::new(
        "example::defaults",
        &language.catalog().expect("explicit standard providers"),
    );
    let mut declaration = module.define_trait("Echo");
    let item = declaration.type_parameter("T").unwrap().ty();
    let method = declaration
        .define_method(MethodDecl::instance("echo"))
        .unwrap();
    declaration
        .method(&method, |method| {
            let key = method.type_parameter("K")?.ty();
            method.parameter("item", item);
            method.parameter("key", key.clone());
            method.returns(key.clone());
            method.bound(key.clone(), language.hash().apply([]));
            method.requires(CallableRequirement::method(
                key,
                language.hash().method("hash")?,
            ));
            Ok(())
        })
        .unwrap();
    declaration
        .bind_default_with(
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
            group.trait_impl(echo.apply([Type::scalar(BuiltinType::String)]), |_| Ok(()))
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
    kagari_runtime::native::module::NativeModule::install_all(
        &kagari_stdlib::modules().unwrap(),
        &mut runtime,
    )
    .unwrap();
    module.install(&mut runtime).unwrap();
}

#[test]
fn native_default_rejects_unproven_operations_and_incompatible_codecs() {
    let language = StandardDeclarations::default();
    for declare_bound in [false, true] {
        let mut module = ModuleBuilder::new(
            "example::default_bounds",
            &language.catalog().expect("explicit standard providers"),
        );
        let mut declaration = module.define_trait("Inspect");
        let method = declaration
            .define_method(MethodDecl::instance("inspect"))
            .unwrap();
        declaration
            .method(&method, |method| {
                let key = method.type_parameter("K")?.ty();
                method.parameter("key", key.clone());
                if declare_bound {
                    method.bound(key.clone(), language.hash().apply([]));
                }
                method.requires(CallableRequirement::method(
                    key,
                    language.hash().method("hash")?,
                ));
                Ok(())
            })
            .unwrap();
        declaration
            .bind_default_with(
                method.clone(),
                NativeBinding::new(vec![Codec::Value, Codec::Value], Codec::Value, |_| {
                    Ok(Value::Unit)
                }),
            )
            .unwrap();
        assert!(
            declaration
                .bind_default_with(
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
    let mut module = ModuleBuilder::new(
        "example::default_codec",
        &language.catalog().expect("explicit standard providers"),
    );
    let mut declaration = module.define_trait("Count");
    let method = declaration
        .define_method(MethodDecl::instance("count").returns(Type::usize()))
        .unwrap();
    declaration
        .bind_default_with(
            method,
            NativeBinding::new(
                vec![Codec::Value],
                Codec::Scalar(Type::i32().abi().clone()),
                |_| Ok(Value::I32(0)),
            ),
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

#[test]
fn full_markdown_survives_builder_completion_and_cross_module_inheritance() {
    let markdown = "Read a value.\n\n# Examples\n\n```kgr\nfn answer() -> i32 { 42 }\n```\n\n- Preserve **Markdown**.";
    let mut provider = ModuleBuilder::new("docs::provider", &DeclarationCatalog::default());
    provider.documentation("# Provider\n\nFull module overview.");
    let mut contract = provider.define_trait("Read");
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
    let declared = provider.to_declaration().unwrap();
    assert_eq!(declared.documentation[read.id()], markdown);
    assert_eq!(declared.documentation[overridden.id()], markdown);
    assert_eq!(
        declared.module_documentation,
        "# Provider\n\nFull module overview."
    );

    let mut consumer = ModuleBuilder::new("docs::consumer", &provider.catalog());
    let mut ty = consumer.define_type("Value");
    ty.documentation("# Value\n\nShared payload.");
    ty.native_storage(NativeStorage::new(|_| Ok(EmptyPayload)))
        .unwrap();
    let value = ty.finish().unwrap();
    consumer
        .implement(value, |group| {
            group.trait_impl(reference.apply([]), |methods| {
                methods.associated_type("Item", Type::i32())?;
                methods.documentation("other", "Implementation-specific **override**.")?;
                assert!(methods.documentation("missing", "invalid").is_err());
                for method in ["read", "other"] {
                    methods.bind_with(
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
                methods.bind_with(
                    method,
                    NativeBinding::new(vec![Codec::Value], Codec::Value, |_| Ok(Value::I32(42))),
                )
            })
        })
        .unwrap();
    let declared = consumer.finish().unwrap().to_declaration().unwrap();
    assert!(
        declared
            .documentation
            .values()
            .any(|text| text == "# Value\n\nShared payload.")
    );
    assert_eq!(
        declared.documentation[&ModuleDecl::method_id(&declared.implementation_id(0), "read")],
        markdown
    );
    assert_eq!(
        declared.documentation[&ModuleDecl::method_id(&declared.implementation_id(0), "other")],
        "Implementation-specific **override**."
    );
    assert_eq!(
        declared.documentation[&ModuleDecl::method_id(&declared.implementation_id(1), "own")],
        markdown
    );
}

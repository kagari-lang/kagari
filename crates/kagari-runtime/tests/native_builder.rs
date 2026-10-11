use kagari_hir::native::render::declaration_source;
use kagari_runtime::{
    Runtime,
    native::{
        binding::{Codec, NativeBinding, NativeResult},
        builder::ModuleBuilder,
        context::CallContext,
        declarations::{FunctionDecl, MethodDecl},
        types::Type,
    },
    value::Value,
};
use kagari_stdlib::{
    catalog as foundation_catalog, declarations::StandardDeclarations, namespaces,
};
use kagari_types::scalar::BuiltinType;

fn add(_cx: &mut CallContext<'_>, left: i32, right: i32) -> NativeResult<i32> {
    Ok(left + right)
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

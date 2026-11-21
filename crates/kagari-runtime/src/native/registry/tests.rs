//! Detached executable declarations are linked independently of their input producer.

use crate::{
    Runtime,
    executor::Executor,
    interpreter_tests::common::compile_with_provider,
    native::{
        assembly::{ModuleDefinition, ModuleSetBuilder, draft::TraitImplementation},
        binding::{Codec, NativeBinding},
        builder::ModuleBuilder,
        catalog::DeclarationCatalog,
        conversion::arguments::ArgumentValues,
        declarations::{FunctionDecl, MethodDecl},
        module::NativeModule,
        types::Type,
    },
    test_support::{bytecode::load_bytecode, native::program::ProgramFixture},
    value::view::ValueView,
};
use kagari_bytecode::{module::BytecodeModule, program::ModuleRef};
use kagari_common::identity::{
    DefinitionKind, map::DefinitionContext, metadata::DefinitionMetadata,
};
use kagari_contract::{
    native_import::NativeImport,
    types::{ConcreteFunctionIdentity, PublicItem},
};
use kagari_types::{
    callable::{CallableImplementation, Signature},
    language::Protocol,
    ty::{GenericBound, Ty},
};

#[test]
fn installed_blanket_admits_closed_forward_calls_and_rejects_missing_evidence() {
    let mut set = ModuleSetBuilder::new(&Default::default()).unwrap();
    set.add(ModuleDefinition::new(Protocol::From.module(), |module| {
        module.package_alias("core");
        let mut traits = Vec::new();
        for (name, member, instance) in [("From", "from", false), ("Into", "into", true)] {
            let id = module.definition(DefinitionKind::Trait, name)?;
            let path = module.definitions().resolve(id).unwrap().to_path();
            let owner = module.reference_trait(path, 1)?;
            let signature = if instance {
                MethodDecl::instance(member).returns(owner.parameter(0)?)
            } else {
                MethodDecl::static_method(member)
                    .parameter("value", owner.parameter(0)?)
                    .returns(owner.receiver()?)
            };
            let method = module.trait_method(id, signature)?;
            module.define_trait(name, 1, |contract, _| {
                contract.methods.push(method);
                Ok(())
            })?;
            traits.push((module.reference_method(&owner, member, 0)?, owner));
        }
        let [(from, forward), (into, reverse)]: [_; 2] = traits.try_into().ok().unwrap();
        module.trait_implementation_with(2, |parameters| {
            let source = parameters[0].clone();
            let dest = parameters[1].clone();
            let applied = forward.apply([source.clone()])?;
            let Ty::Trait(required) = applied.abi() else {
                unreachable!()
            };
            Ok(TraitImplementation {
                receiver: source,
                interface: reverse.apply([dest.clone()])?,
                bounds: vec![GenericBound {
                    ty: dest.abi().clone(),
                    constraints: vec![required.clone()],
                }],
                bindings: vec![],
            }
            .with_script_method(into, move |scope| {
                scope.trait_call(&from, dest, applied, [], [scope.parameter(0)?])
            }))
        })
    }))
    .unwrap();
    let modules = set.finish().unwrap().into_modules().unwrap();
    let provider = &modules[0];
    let mut runtime = Runtime::default();
    provider.install(&mut runtime).unwrap();
    // No calls, no imports and no inconsistent product-side references. Only the
    // independent installation knows this module is missing an implementation.
    let input = ProgramFixture {
        root: ModuleRef::new(0),
        modules: vec![BytecodeModule {
            identity: Protocol::From.module(),
            public_items: provider
                .declaration()
                .traits
                .iter()
                .cloned()
                .map(PublicItem::Trait)
                .collect(),
            ..Default::default()
        }],
        exports: Default::default(),
    }
    .metadata(provider.definitions().clone(), &Default::default())
    .unwrap();
    let error = load_bytecode(&mut runtime, "incomplete", input).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("conversion blanket differs from its installed implementation"),
        "{error}"
    );
    assert_eq!(runtime.modules().loaded_count(), 0);

    let input = compile_with_provider(
        include_str!("../../../tests/fixtures/native/installed-conversion.kgr"),
        Some(provider),
    );
    let loaded = load_bytecode(&mut runtime, "complete", input).unwrap();
    let selected = runtime.select_program(&loaded).unwrap();
    let (owner, entry) = selected.entry_function().unwrap();
    let result = Executor::new(&mut runtime, &owner, entry, ArgumentValues::scalars(&[]))
        .unwrap()
        .run(|runtime, stack, value| stack.retain_result(runtime, value))
        .unwrap();
    assert_eq!(result.as_i64(), Some(42));
}

fn provider(wide: bool) -> NativeModule {
    let mut builder =
        ModuleBuilder::new("example::detached", &DeclarationCatalog::default()).unwrap();
    let function = builder
        .define_function(FunctionDecl::new("answer").returns(if wide {
            Type::i64()
        } else {
            Type::i32()
        }))
        .unwrap();
    if wide {
        builder
            .bind(
                function,
                NativeBinding::new(vec![], Codec::Exact(Type::i64()), |_| {
                    Ok(ValueView::from(42_i64))
                }),
            )
            .unwrap();
    } else {
        builder
            .bind(
                function,
                NativeBinding::new(vec![], Codec::Exact(Type::i32()), |_| {
                    Ok(ValueView::from(42_i32))
                }),
            )
            .unwrap();
    }
    let original = builder.finish().unwrap().to_declaration();
    let catalog = DeclarationCatalog::default();
    let imported = original
        .import_into(catalog.types.context(), &Default::default())
        .unwrap();
    let mut module = imported.into_records();
    let CallableImplementation::Native(entry) = module.functions[0].implementation else {
        unreachable!()
    };
    let mut alias = module.functions[0].clone();
    alias.name = "alias".into();
    module.functions.push(alias);
    let binding = if wide {
        NativeBinding::new(vec![], Codec::Exact(Type::i64()), |_| {
            Ok(ValueView::from(42_i64))
        })
    } else {
        NativeBinding::new(vec![], Codec::Exact(Type::i32()), |_| {
            Ok(ValueView::from(42_i32))
        })
    };
    NativeModule::checked(
        module,
        vec![(entry, binding)],
        Default::default(),
        vec![],
        &catalog,
    )
    .unwrap()
}

#[test]
fn demanded_binding_matches_the_checked_declaration_after_detached_loading() {
    let expected = provider(false).to_declaration();
    let context = DefinitionContext::new().unwrap();
    let expected = expected.import_into(&context, &Default::default()).unwrap();
    expected.records().collect_definitions(&context).unwrap();
    let mut declarations = expected
        .records()
        .native_declarations(&context.snapshot())
        .unwrap();
    for declaration in &mut declarations {
        declaration.function.implementation =
            CallableImplementation::Native(declaration.declaration);
    }
    assert_eq!(declarations.len(), 2);
    let imports = declarations
        .iter()
        .map(|declaration| NativeImport {
            instance: ConcreteFunctionIdentity {
                declaration: declaration.declaration,
                arguments: vec![],
            },
            binding: declaration.declaration,
            signature: Signature {
                params: vec![],
                result: declaration.function.return_type.clone(),
            },
            receiver_access: None,
            generic: None,
            result_adapter: None,
            callables: vec![],
            requirements: vec![],
        })
        .collect();
    let program = ProgramFixture {
        root: ModuleRef::new(0),
        modules: vec![BytecodeModule {
            identity: expected.records().identity.clone(),
            native_declarations: declarations,
            native_imports: imports,
            ..Default::default()
        }],
        exports: Default::default(),
    }
    .build(&context);
    let program =
        DefinitionMetadata::checked(context.snapshot(), program, &Default::default()).unwrap();
    let mut runtime = Runtime::default();
    let error = load_bytecode(&mut runtime, "missing", program.clone()).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("native declaration is not installed"),
        "{error}"
    );
    assert_eq!(runtime.modules().loaded_count(), 0);
    provider(true).install(&mut runtime).unwrap();
    let error = load_bytecode(&mut runtime, "mismatch", program.clone()).unwrap_err();
    assert!(
        error.to_string().contains("native declaration differs"),
        "{error}"
    );
    assert_eq!(runtime.modules().loaded_count(), 0);
    let mut runtime = Runtime::default();
    provider(false).install(&mut runtime).unwrap();
    load_bytecode(&mut runtime, "matched", program).unwrap();
    assert_eq!(runtime.modules().loaded_count(), 1);
}

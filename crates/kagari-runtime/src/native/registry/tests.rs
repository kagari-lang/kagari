//! Detached executable declarations are linked independently of their input producer.

use crate::{
    Runtime,
    native::{
        binding::{Codec, NativeBinding},
        builder::ModuleBuilder,
        catalog::DeclarationCatalog,
        declarations::FunctionDecl,
        module::NativeModule,
        types::Type,
    },
    test_support::{bytecode::load_bytecode, native::program::ProgramFixture},
    value::view::ValueView,
};
use kagari_bytecode::{module::BytecodeModule, program::ModuleRef};
use kagari_common::identity::{map::DefinitionContext, metadata::DefinitionMetadata};
use kagari_contract::{native_import::NativeImport, types::ConcreteFunctionIdentity};
use kagari_types::callable::{CallableImplementation, Signature};

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

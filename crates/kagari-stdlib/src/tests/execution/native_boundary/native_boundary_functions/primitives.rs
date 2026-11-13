//! Primitive authority follows the installed implementation across shared portable code.
use crate::{
    declarations::StandardDeclarations,
    tests::execution::native_boundary::{compile_program, support::round_trip_program},
};
use kagari_runtime::{
    Runtime, RuntimeConfig,
    execution::memory::GcHeapConfig,
    module::{VerifiedProgram, entry::ProgramEntry},
    native::{
        binding::{Codec, NativeBinding},
        builder::ModuleBuilder,
        declarations::FunctionDecl,
        module::NativeModule,
        primitive::NativePrimitive,
        types::Type,
    },
    value::view::ValueView,
};

use kagari_common::identity::map::DefinitionContext;

use kagari_types::scalar::BuiltinType;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[test]
fn runtime_primitives_follow_installed_bodies_and_exact_signatures() {
    let catalog = StandardDeclarations::default().catalog().unwrap();
    let string = Type::scalar(BuiltinType::String);
    let primitive = || NativeBinding::primitive(NativePrimitive::StringByteLength);
    // String and u64 are semantic types, not merely pointer/integer layouts.
    for (argument, result) in [(Type::i32(), Type::u64()), (string.clone(), Type::i64())] {
        let mut builder = ModuleBuilder::new("example::primitives", &catalog).unwrap();
        let function = builder
            .define_function(
                FunctionDecl::new("count")
                    .parameter("text", argument)
                    .returns(result),
            )
            .unwrap();
        assert!(builder.bind(function, primitive()).is_err());
    }

    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let callback = NativeBinding::new(
        vec![Codec::Exact(Type::scalar(BuiltinType::String))],
        Codec::Exact(Type::u64()),
        move |_| {
            observed.fetch_add(1, Ordering::Relaxed);
            Ok(ValueView::from(41u64))
        },
    );
    let mut modules = Vec::new();
    for binding in [primitive(), callback] {
        let mut builder = ModuleBuilder::new("example::primitives", &catalog).unwrap();
        let function = builder
            .define_function(
                FunctionDecl::new("count")
                    .parameter("text", string.clone())
                    .returns(Type::u64()),
            )
            .unwrap();
        let id = function
            .definitions()
            .resolve(function.id())
            .unwrap()
            .to_path();
        builder.bind(function, binding).unwrap();
        modules.push((builder.finish().unwrap(), id));
    }
    let source =
        include_str!("../../../../../tests/fixtures/native/function-string-primitives.kgr");
    let context = DefinitionContext::new().unwrap();
    let verified = VerifiedProgram::new(
        round_trip_program(compile_program(source, Some(&modules[0].0))),
        &context,
    )
    .unwrap();
    // The same verified prepared instructions link separately in both runtimes.
    // Neither equal names/signatures nor the first link may select the second body.
    for (index, (module, function)) in modules.iter().enumerate() {
        let mut runtime = Runtime::new(RuntimeConfig {
            gc: GcHeapConfig {
                collection_threshold: Some(1),
                ..Default::default()
            },
            ..Default::default()
        });
        NativeModule::install_all(&crate::modules().unwrap(), &mut runtime).unwrap();
        module.install(&mut runtime).unwrap();
        let owner = runtime
            .load_verified_program("primitives", verified.clone())
            .unwrap();
        let _owner_program = owner;
        let owner = runtime.select_program(&_owner_program).unwrap();
        let mut vm = runtime;
        let report = vm
            .execute_module(&owner, ProgramEntry::Main, vm.execution_options())
            .unwrap();
        assert_eq!(
            report.return_value.as_u64(),
            Some(if index == 0 { 8 } else { 82 })
        );
        let bound = vm
            .bind_function_declaration::<(String,), usize>(&_owner_program.root(), function)
            .unwrap();
        assert_eq!(
            vm.call(&bound, ("aé文".to_owned(),)).unwrap(),
            if index == 0 { 6 } else { 41 }
        );
        assert_eq!(vm.resources().counters().current_call_depth, 0);
    }
    assert_eq!(calls.load(Ordering::Relaxed), 4);
}

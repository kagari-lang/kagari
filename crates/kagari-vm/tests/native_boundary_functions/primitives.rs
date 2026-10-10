//! Primitive authority follows the installed implementation across shared portable code.
use crate::compile_program;
use kagari_bytecode::artifact::KbcArtifact;
use kagari_runtime::{
    Runtime, RuntimeConfig,
    gc::GcHeapConfig,
    module::VerifiedProgram,
    native::{
        binding::{Codec, NativeBinding},
        builder::ModuleBuilder,
        declarations::FunctionDecl,
        module::NativeModule,
        primitive::NativePrimitive,
        types::Type,
    },
    value::Value,
};
use kagari_stdlib::declarations::StandardDeclarations;
use kagari_types::{scalar::BuiltinType, ty::Ty};
use kagari_vm::vm::Vm;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[test]
fn runtime_primitives_follow_installed_bodies_and_exact_signatures() {
    let catalog = StandardDeclarations::default().catalog().unwrap();
    let string = Type::scalar(BuiltinType::String);
    let primitive = || NativeBinding::primitive(NativePrimitive::StringByteLength);
    // String and usize are semantic types, not merely pointer/integer layouts.
    for (argument, result) in [(Type::i32(), Type::usize()), (string.clone(), Type::u64())] {
        let mut builder = ModuleBuilder::new("example::primitives", &catalog);
        let function = builder
            .define_function(
                FunctionDecl::new("count")
                    .parameter("text", argument)
                    .returns(result),
            )
            .unwrap();
        assert!(builder.bind_with(function, primitive()).is_err());
    }

    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let callback = NativeBinding::new(
        vec![Codec::Scalar(Ty::Builtin(BuiltinType::String))],
        Codec::Scalar(Ty::Builtin(BuiltinType::USize)),
        move |_| {
            observed.fetch_add(1, Ordering::Relaxed);
            Ok(Value::U64(41))
        },
    );
    let mut modules = Vec::new();
    for binding in [primitive(), callback] {
        let mut builder = ModuleBuilder::new("example::primitives", &catalog);
        let function = builder
            .define_function(
                FunctionDecl::new("count")
                    .parameter("text", string.clone())
                    .returns(Type::usize()),
            )
            .unwrap();
        let id = function.id().clone();
        builder.bind_with(function, binding).unwrap();
        modules.push((builder.finish().unwrap(), id));
    }
    let source = r#"
        use example::primitives::count;
        pub fn main() -> usize {
            val callable = |text: String| count(text);
            count("discarded");
            count("aé文") + callable("é")
        }
    "#;
    let artifact = KbcArtifact::from_program(
        compile_program(source, Some(&modules[0].0)),
        Default::default(),
    )
    .unwrap();
    let verified = VerifiedProgram::new(
        KbcArtifact::from_bytes(&artifact.to_bytes().unwrap())
            .unwrap()
            .program,
    )
    .unwrap();
    // The same verified prepared instructions link separately in both runtimes.
    // Neither equal names/signatures nor the first link may select the second body.
    for (index, (module, function)) in modules.iter().enumerate() {
        let mut runtime = Runtime::new(RuntimeConfig {
            gc: GcHeapConfig {
                collection_threshold: Some(1),
            },
            ..Default::default()
        });
        NativeModule::install_all(&kagari_stdlib::modules().unwrap(), &mut runtime).unwrap();
        module.install(&mut runtime).unwrap();
        let owner = runtime
            .load_verified_program("primitives", verified.clone())
            .unwrap();
        let vm = Vm::new(runtime);
        let report = vm.execute(&owner, "main").unwrap();
        assert_eq!(
            report.return_value.value(vm.runtime().gc()),
            Some(Value::U64(if index == 0 { 8 } else { 82 }))
        );
        let bound = vm
            .runtime()
            .bind_function_declaration::<(String,), usize>(&owner, function)
            .unwrap();
        assert_eq!(
            vm.call(&bound, ("aé文".to_owned(),)).unwrap(),
            if index == 0 { 6 } else { 41 }
        );
        assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
    }
    assert_eq!(calls.load(Ordering::Relaxed), 4);
}

//! Primitive authority follows the installed implementation across shared portable code.
use super::fixture;
use crate::compile_program;
use kagari_bytecode::artifact::KbcArtifact;
use kagari_runtime::{
    Runtime, RuntimeConfig,
    error::RuntimeErrorKind,
    gc::GcHeapConfig,
    module::VerifiedProgram,
    native::{
        binding::{Codec, NativeBinding},
        builder::ModuleBuilder,
        collections::vector::ScriptVec,
        declarations::FunctionDecl,
        module::NativeModule,
        primitive::NativePrimitive,
        typed::NativeContext,
        types::Type,
    },
    value::Value,
};
use kagari_stdlib::declarations::StandardDeclarations;
use kagari_types::{scalar::BuiltinType, ty::Ty};
use kagari_vm::{error::VmError, vm::Vm};
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

#[test]
fn vector_primitives_preserve_element_access_aliases_and_bounds() {
    let catalog = StandardDeclarations::default().catalog().unwrap();
    let vector = || StandardDeclarations::default().vec(Type::i32());
    let array = || Type::from_semantic(Ty::Array(Box::new(Ty::Builtin(BuiltinType::I32))));
    let list = || {
        StandardDeclarations::default()
            .list()
            .apply([Type::i32()])
            .ty()
    };
    for (body, receiver, value, result) in [
        (NativePrimitive::VecIndex, array(), None, Type::i32()),
        (
            NativePrimitive::VecSet,
            array(),
            Some(Type::i32()),
            Type::unit(),
        ),
        (NativePrimitive::VecIndex, vector(), None, Type::bool()),
        (
            NativePrimitive::VecSet,
            list(),
            Some(Type::i32()),
            Type::unit(),
        ),
        (
            NativePrimitive::VecSet,
            vector(),
            Some(Type::bool()),
            Type::unit(),
        ),
        (
            NativePrimitive::VecSetFluent,
            vector(),
            Some(Type::i32()),
            list(),
        ),
    ] {
        let mut builder = ModuleBuilder::new("example::vector", &catalog);
        let mut declaration = FunctionDecl::new("operation")
            .parameter("values", receiver)
            .parameter("index", Type::usize())
            .returns(result);
        if let Some(value) = value {
            declaration = declaration.parameter("value", value);
        }
        let function = builder.define_function(declaration).unwrap();
        assert!(
            builder
                .bind_with(function, NativeBinding::primitive(body))
                .is_err()
        );
    }
    let mut builder = ModuleBuilder::new("example::vector", &catalog);
    for (name, body, write, result) in [
        ("read", NativePrimitive::VecIndex, false, Type::i32()),
        ("replace", NativePrimitive::VecSet, true, Type::unit()),
        (
            "replace_fluent",
            NativePrimitive::VecSetFluent,
            true,
            vector(),
        ),
    ] {
        let mut declaration = FunctionDecl::new(name)
            .parameter("values", vector())
            .parameter("index", Type::usize())
            .returns(result);
        if write {
            declaration = declaration.parameter("value", Type::i32());
        }
        let function = builder.define_function(declaration).unwrap();
        builder
            .bind_with(function, NativeBinding::primitive(body))
            .unwrap();
    }
    let module = builder.finish().unwrap();
    let (vm, owner) = fixture(
        r#"
        use example::vector::{read, replace, replace_fluent};
        pub fn make() -> Vec<i32> { Vec::from([1, 2]) }
        pub fn update(values: Vec<i32>) -> i32 {
            val alias = values;
            replace(values, 0, 7);
            val returned = replace_fluent(values, 1, 8);
            read(alias, 0) + read(returned, 1)
        }
        pub fn failed_write(values: Vec<i32>) {
            replace(values, 0, 9);
            replace(values, 99, 3);
        }
        pub fn failed_read(values: Vec<i32>) -> i32 { read(values, 99) }
    "#,
        Some(&module),
    );
    let values: ScriptVec<i32> = vm.execute_typed(&owner, "make", ()).unwrap();
    assert_eq!(
        vm.execute_typed::<_, i32>(&owner, "update", (values.clone(),))
            .unwrap(),
        15
    );
    let error = vm
        .execute_typed::<_, ()>(&owner, "failed_write", (values.clone(),))
        .unwrap_err();
    assert!(
        matches!(error.cause(), VmError::RuntimeError(error) if error.kind() == RuntimeErrorKind::IndexOutOfBounds)
    );
    let error = vm
        .execute_typed::<_, i32>(&owner, "failed_read", (values.clone(),))
        .unwrap_err();
    assert!(
        matches!(error.cause(), VmError::RuntimeError(error) if error.kind() == RuntimeErrorKind::IndexOutOfBounds)
    );
    let mut cx = NativeContext::new(vm.runtime(), &owner).unwrap();
    assert_eq!(values.get(&mut cx, 0).unwrap(), Some(9));
    assert_eq!(values.get(&mut cx, 1).unwrap(), Some(8));
    assert!(
        vm.execute_typed::<_, i32>(&owner, "update", (values.read_only(),))
            .is_err()
    );
    assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
}

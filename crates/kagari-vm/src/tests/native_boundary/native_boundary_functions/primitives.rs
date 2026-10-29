//! Primitive authority follows the installed implementation across shared portable code.
use super::fixture;
use crate::tests::native_boundary::{compile_program, support::round_trip_program};
use crate::{error::VmError, vm::Vm};
use kagari_common::identity::map::DefinitionContext;
use kagari_runtime::{
    Runtime, RuntimeConfig,
    error::{RuntimeError, RuntimeErrorKind},
    gc::GcHeapConfig,
    module::VerifiedProgram,
    module::entry::ProgramEntry,
    native::{
        binding::{Codec, NativeBinding, NativeResult},
        builder::ModuleBuilder,
        declarations::FunctionDecl,
        module::NativeModule,
        primitive::NativePrimitive,
        typed::NativeContext,
        types::Type,
    },
    value::Value,
};
use kagari_stdlib::collections::vector::ScriptVec;
use kagari_stdlib::declarations::StandardDeclarations;
use kagari_types::{scalar::BuiltinType, ty::Ty};
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
    let callback = unsafe {
        NativeBinding::new_transport(
            vec![Codec::Exact(Type::scalar(BuiltinType::String))],
            Codec::Exact(Type::u64()),
            move |_| {
                observed.fetch_add(1, Ordering::Relaxed);
                Ok(Value::U64(41))
            },
        )
    };
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
    let source = r#"
        use example::primitives::count;
        pub fn main() -> u64 {
            val callable = |text: String| count(text);
            count("discarded");
            count("aé文") + callable("é")
        }
    "#;
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
        NativeModule::install_all(&kagari_stdlib::modules().unwrap(), &mut runtime).unwrap();
        module.install(&mut runtime).unwrap();
        let owner = runtime
            .load_verified_program("primitives", verified.clone())
            .unwrap();
        let _owner_program = owner;
        let owner = runtime.select_program(&_owner_program).unwrap();
        let vm = Vm::new(runtime);
        let report = vm.execute(&owner, ProgramEntry::Main).unwrap();
        assert_eq!(
            report
                .return_value
                .inspect(vm.runtime(), |view| view.as_u64().map(Value::U64))
                .ok(),
            Some(Value::U64(if index == 0 { 8 } else { 82 }))
        );
        let bound = vm
            .runtime()
            .bind_function_declaration::<(String,), usize>(&_owner_program.root(), function)
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
fn vector_provider_preserves_element_access_aliases_and_bounds() {
    let catalog = StandardDeclarations::default().catalog().unwrap();
    let vector = || StandardDeclarations::default().vec(Type::i32()).unwrap();
    let definitions = DefinitionContext::new().unwrap().snapshot();
    let array = || {
        Type::from_semantic(
            Ty::Array(Box::new(Ty::Builtin(BuiltinType::I32))),
            &definitions,
        )
        .unwrap()
    };
    let list = || {
        StandardDeclarations::default()
            .list()
            .apply([Type::i32()])
            .unwrap()
            .ty()
    };
    for (receiver, argument) in [(array(), "[1, 2]"), (list(), "Vec::from([1, 2])")] {
        let mut builder = ModuleBuilder::new("example::vector", &catalog).unwrap();
        let function = builder
            .define_function(
                FunctionDecl::new("operation")
                    .parameter("values", receiver)
                    .parameter("index", Type::u64())
                    .returns(Type::i32()),
            )
            .unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        // Contextual Rust mappings are checked against the installed concrete
        // invocation before conversion or callback effects, including interfaces.
        builder
            .bind(
                function,
                move |cx: &mut NativeContext<'_>,
                      (values, index): (ScriptVec<i32>, u64)|
                      -> NativeResult<i32> {
                    observed.fetch_add(1, Ordering::SeqCst);
                    values.get(cx, index as usize)?.ok_or_else(|| {
                        RuntimeError::new(RuntimeErrorKind::IndexOutOfBounds, "vector index")
                    })
                },
            )
            .unwrap();
        let module = builder.finish().unwrap();
        let (vm, owner) = fixture(
            &format!(
                "use example::vector::operation; fn main() -> i32 {{ operation({argument}, 0) }}"
            ),
            Some(&module),
        );
        let loaded = vm.runtime().select_program(&owner).unwrap();
        let error = vm.execute(&loaded, ProgramEntry::Main).unwrap_err();
        assert!(
            matches!(error.cause(), VmError::RuntimeError(error) if error.kind() == RuntimeErrorKind::ModuleValidation)
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
    }
    let mut builder = ModuleBuilder::new("example::vector", &catalog).unwrap();
    let read = builder
        .define_function(
            FunctionDecl::new("read")
                .parameter("values", vector())
                .parameter("index", Type::u64())
                .returns(Type::i32()),
        )
        .unwrap();
    builder
        .bind(
            read,
            |cx: &mut NativeContext<'_>,
             (values, index): (ScriptVec<i32>, u64)|
             -> NativeResult<i32> {
                values.get(cx, index as usize)?.ok_or_else(|| {
                    RuntimeError::new(RuntimeErrorKind::IndexOutOfBounds, "vector index")
                })
            },
        )
        .unwrap();
    let replace = builder
        .define_function(
            FunctionDecl::new("replace")
                .parameter("values", vector())
                .parameter("index", Type::u64())
                .parameter("value", Type::i32()),
        )
        .unwrap();
    builder
        .bind(
            replace,
            |cx: &mut NativeContext<'_>, (values, index, value): (ScriptVec<i32>, u64, i32)| {
                values.set(cx, index as usize, value)
            },
        )
        .unwrap();
    let fluent = builder
        .define_function(
            FunctionDecl::new("replace_fluent")
                .parameter("values", vector())
                .parameter("index", Type::u64())
                .parameter("value", Type::i32())
                .returns(vector()),
        )
        .unwrap();
    builder
        .bind(
            fluent,
            |cx: &mut NativeContext<'_>,
             (values, index, value): (ScriptVec<i32>, u64, i32)|
             -> NativeResult<ScriptVec<i32>> {
                values.set(cx, index as usize, value)?;
                Ok(values)
            },
        )
        .unwrap();
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
    let _owner_program = owner;
    let owner = vm.runtime().select_program(&_owner_program).unwrap();
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

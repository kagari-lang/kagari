use crate::{error::VmError, vm::Vm};
use kagari_bytecode::program::BytecodeProgram;
use kagari_common::identity::metadata::DefinitionMetadata;
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_hir::analysis::AnalysisDatabase;

use kagari_runtime::{
    Runtime,
    error::{RuntimeError, RuntimeErrorKind},
    module::{Program, entry::ProgramEntry},
    native::{
        binding::{Codec, NativeBinding, NativeResult},
        builder::ModuleBuilder,
        context::CallContext,
        conversion::context::ConversionContext,
        declarations::FunctionDecl,
        functions::NativeCallback,
        module::NativeModule,
        objects::Object,
        payload::Opaque,
        storage::{
            NativePayload, NativeStorage,
            scan::{NativeScanKind, ScanBudget, ScanStatus},
            value::StoredValue,
        },
        storage_type::StorageType,
        typed::NativeContext,
        types::Type,
        value_handle::ScriptValue,
    },
    resource::HeapCharge,
    value::Value,
};
use kagari_source::source_database::{SourceDatabase, SourceLayer};

use std::sync::Arc;
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};
use {
    kagari_stdlib as foundation,
    kagari_stdlib::{
        collections::{map::ScriptMap, set::ScriptSet, vector::ScriptVec},
        declarations::StandardDeclarations,
    },
};

mod native_boundary_artifacts;
mod native_boundary_callbacks;
mod native_boundary_control;
mod native_boundary_conversion;
mod native_boundary_enums;
mod native_boundary_functions;
mod native_boundary_gc;
mod native_boundary_hash_handles;
mod native_boundary_host;
mod native_boundary_interfaces;
mod native_boundary_payload;
mod native_boundary_references;
mod native_boundary_resources;
mod native_boundary_sessions;
mod native_boundary_storage;
mod native_boundary_transfer;
mod support;

fn compile_program(
    text: &str,
    module: Option<&NativeModule>,
) -> DefinitionMetadata<BytecodeProgram> {
    let modules = foundation::modules().unwrap();
    compile_with_modules(
        text,
        modules
            .iter()
            .filter(|installed| {
                module.is_none_or(|module| {
                    module.declaration().identity != installed.declaration().identity
                })
            })
            .chain(module),
    )
}

/// Reload fixtures include only their providers, not unrelated protected foundation code.
pub(super) fn compile_without_foundation(
    text: &str,
    module: Option<&NativeModule>,
) -> DefinitionMetadata<BytecodeProgram> {
    compile_with_modules(text, module)
}

fn compile_with_modules<'a>(
    text: &str,
    modules: impl IntoIterator<Item = &'a NativeModule>,
) -> DefinitionMetadata<BytecodeProgram> {
    let mut sources = SourceDatabase::default();
    let root = sources
        .set("main.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut analysis = AnalysisDatabase::default();
    analysis.set_native_modules(
        modules
            .into_iter()
            .map(|module| Arc::new(module.to_declaration()))
            .collect(),
    );
    let snapshot = analysis
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let checked = snapshot.check_program(root, &Default::default()).unwrap();
    let mir = lower_program_to_mir(&checked, &Default::default()).unwrap();
    lower_program_to_bytecode(&mir, &Default::default())
        .map(|program| program.into_unverified())
        .unwrap()
}

fn compile(text: &str, module: Option<&NativeModule>) -> (Vm, Program) {
    let program = compile_program(text, module);
    let mut runtime = Runtime::default();
    NativeModule::install_all(&kagari_stdlib::modules().unwrap(), &mut runtime).unwrap();
    if let Some(module) = module {
        module.install(&mut runtime).unwrap();
    }
    let loaded = runtime.load_program("boundary", program).unwrap();
    (Vm::new(runtime), loaded)
}

// Constant caches belong to a published/pinned program, independently of host
// value roots. Finish version-specific checks and drop bindings before retiring it.
fn publish_unused_version(vm: &Vm, active: &Program) {
    let selected = vm.runtime().select_program(active).unwrap();
    let candidate = vm
        .runtime()
        .stage_reload_verified_program(active, None, selected.verified_program().clone())
        .unwrap();
    vm.runtime().publish_staged_reload(candidate).unwrap();
}

#[test]
fn scalar_function_executes_through_the_checked_registration() {
    let mut module = ModuleBuilder::new(
        "example::math",
        &StandardDeclarations::default()
            .catalog()
            .expect("explicit standard providers"),
    )
    .unwrap();
    let add = module
        .define_function(
            FunctionDecl::new("add")
                .parameter("a", Type::i32())
                .parameter("b", Type::i32())
                .returns(Type::i32()),
        )
        .unwrap();
    module
        .bind(
            add,
            |_cx: &mut CallContext<'_>, a: i32, b: i32| -> NativeResult<i32> { Ok(a + b) },
        )
        .unwrap();
    let module = module.finish().unwrap();
    let (vm, loaded) = compile(
        "use example::math::add; fn main() -> i32 { add(20, 22) }",
        Some(&module),
    );
    let _loaded_program = loaded;
    let loaded = vm.runtime().select_program(&_loaded_program).unwrap();
    assert_eq!(
        vm.execute(&loaded, ProgramEntry::Main)
            .unwrap()
            .return_value
            .inspect(vm.runtime(), |view| view.as_i32().map(Value::I32))
            .expect("retained execution result"),
        Value::I32(42)
    );
}

#[test]
fn raw_result_contract_failure_releases_the_execution_scope() {
    let mut module = ModuleBuilder::new(
        "example::bad",
        &StandardDeclarations::default()
            .catalog()
            .expect("explicit standard providers"),
    )
    .unwrap();
    let function = module
        .define_function(FunctionDecl::new("broken").returns(Type::i32()))
        .unwrap();
    module
        .bind(function, unsafe {
            NativeCallback::new(|_cx: &mut CallContext<'_>| -> NativeResult<Value> {
                Ok(Value::Bool(true))
            })
        })
        .unwrap();
    let module = module.finish().unwrap();
    let (vm, loaded) = compile(
        "use example::bad::broken; fn main() -> i32 { broken() } pub fn healthy() -> i32 { 42 }",
        Some(&module),
    );
    let _loaded_program = loaded;
    let loaded = vm.runtime().select_program(&_loaded_program).unwrap();
    assert!(vm.execute(&loaded, ProgramEntry::Main).is_err());
    assert_eq!(
        vm.execute(&loaded, "healthy")
            .unwrap()
            .return_value
            .inspect(vm.runtime(), |view| view.as_i32().map(Value::I32))
            .expect("retained execution result"),
        Value::I32(42)
    );
}

#[test]
fn foundation_defaults_execute_without_optional_modules() {
    let (vm, loaded) = compile(
        r#"use std::collections::{HashMap, HashSet};

        fn main() -> i32 {
            val values = Vec::new();
            values.push(20); values.push(22);
            val map = HashMap::new(); map.insert(1, values[0]); map.insert(2, values[1]);
            val set = HashSet::new(); set.insert(1); set.insert(1);
            if values.len() == (2 as u64) && map.len() == (2 as u64) && set.len() == (1 as u64) && map.contains_key(2) {
                var total = 0; for value in values { total = total + value; } total
            } else { 0 }
        }
    "#,
        None,
    );
    let _loaded_program = loaded;
    let loaded = vm.runtime().select_program(&_loaded_program).unwrap();
    assert_eq!(
        vm.execute(&loaded, ProgramEntry::Main)
            .unwrap()
            .return_value
            .inspect(vm.runtime(), |view| view.as_i32().map(Value::I32))
            .expect("retained execution result"),
        Value::I32(42)
    );
}

#[test]
fn custom_hash_collisions_use_selected_script_methods() {
    let (vm, loaded) = compile(
        r#"use std::collections::{HashMap};
use std::hash::{Hash};

        struct Key { val value: i32 }
        impl PartialEq for Key { fn eq(self, other: Key) -> bool { self.value == other.value } }
        impl Eq for Key {}
        impl Hash for Key { fn hash(self) -> i64 { 7 } }
        fn main() -> i32 {
            val map = HashMap::new();
            map.insert(Key { value: 1 }, 20); map.insert(Key { value: 2 }, 22);
            map.insert(Key { value: 1 }, 21);
            val second = Key { value: 2 }; val first = Key { value: 1 };
            val found = map.get(first);
            if map.len() == (2 as u64) && map.contains_key(second) {
                match found { Some(value) => value + 21, None => 0 }
            } else { 0 }
        }
    "#,
        None,
    );
    let _loaded_program = loaded;
    let loaded = vm.runtime().select_program(&_loaded_program).unwrap();
    assert_eq!(
        vm.execute(&loaded, ProgramEntry::Main)
            .unwrap()
            .return_value
            .inspect(vm.runtime(), |view| view.as_i32().map(Value::I32))
            .expect("retained execution result"),
        Value::I32(42)
    );
}

#[test]
fn selected_script_callbacks_return_directly_to_a_rust_loop() {
    use kagari_runtime::native::declarations::{CallableRequirement, MethodDecl};
    let mut module = ModuleBuilder::new(
        "example::calls",
        &StandardDeclarations::default()
            .catalog()
            .expect("explicit standard providers"),
    )
    .unwrap();
    let mut step = module.define_trait("Step").unwrap();
    step.define_method(
        MethodDecl::instance("step")
            .parameter("value", Type::i32())
            .returns(Type::i32()),
    )
    .unwrap();
    let step = step.finish().unwrap();
    let repeat = module
        .define_function(FunctionDecl::new("repeat").returns(Type::i32()))
        .unwrap();
    let selected = module
        .function(&repeat, |function| {
            let item = function.type_parameter("T")?;
            function.parameter("item", item.ty())?;
            function.parameter("count", Type::i32())?;
            function.bound(item.ty(), step.apply([])?)?;
            function.requires(CallableRequirement::method(
                item.ty(),
                step.method("step")?,
            )?)
        })
        .unwrap();
    module
        .bind(repeat, unsafe {
            NativeBinding::new_transport(
                vec![Codec::Value, Codec::Exact(Type::i32())],
                Codec::Exact(Type::i32()),
                move |cx| {
                    // SAFETY: The active call retains this input until its copy is published or consumed.
                    let receiver = cx.argument_transport(0)?;
                    // SAFETY: The active call retains this input until its copy is published or consumed.
                    let Value::I32(count) = cx.argument_transport(1)? else {
                        unreachable!()
                    };
                    let target = cx.selected(&selected)?;
                    let mut value = Value::I32(0);
                    for _ in 0..count {
                        cx.poll()?;
                        // SAFETY: The native argument frame and local working roots protect inputs;
                        // results are decoded, rooted or returned before another callback.
                        value = cx.call_values_transport(target, &[receiver, value])?;
                    }
                    Ok(value)
                },
            )
        })
        .unwrap();
    let module = module.finish().unwrap();
    let (vm, loaded) = compile(
        r#"
        use example::calls::{Step, repeat};
        struct Counter { val amount: i32 }
        impl Step for Counter { fn step(self, value: i32) -> i32 { value + self.amount } }
        fn main() -> i32 { repeat(Counter { amount: 14 }, 3) }
        trait Runner { fn run<T: Step>(self, item: T) -> i32 { repeat(item, 3) } }
        struct Source {}
        impl Runner for Source {}
        pub fn shared() -> i32 { val source: Runner = Source {}; source.run(Counter { amount: 14 }) }
    "#,
        Some(&module),
    );
    let _loaded_program = loaded;
    let loaded = vm.runtime().select_program(&_loaded_program).unwrap();
    for entry in [ProgramEntry::Main, ProgramEntry::Export("shared")] {
        assert_eq!(
            vm.execute(&loaded, entry)
                .unwrap()
                .return_value
                .inspect(vm.runtime(), |view| view.as_i32().map(Value::I32))
                .expect("retained execution result"),
            Value::I32(42)
        );
    }
}

#[derive(Debug)]
struct TracedCounter {
    values: StoredValue,
    contract: Option<StorageType>,
    dropped: Arc<AtomicUsize>,
}

unsafe impl NativePayload for TracedCounter {
    type ScanState = ();
    const SCAN_KIND: NativeScanKind = NativeScanKind::Indivisible;

    fn begin_scan(&self) {}

    fn scan<'payload>(
        &'payload self,
        _: &mut Self::ScanState,
        budget: &mut ScanBudget,
        visit: &mut dyn FnMut(&'payload StoredValue),
    ) -> ScanStatus {
        budget.charge(1);
        visit(&self.values);

        ScanStatus::Complete
    }

    fn charge(&self) -> HeapCharge {
        HeapCharge::new(1, 0)
    }
}

impl Drop for TracedCounter {
    fn drop(&mut self) {
        self.dropped.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn registered_nominal_payload_traces_children_and_drops_with_the_heap() {
    let dropped = Arc::new(AtomicUsize::new(0));
    let factory_dropped = dropped.clone();
    let mut builder = ModuleBuilder::new(
        "example::objects",
        &StandardDeclarations::default()
            .catalog()
            .expect("explicit standard providers"),
    )
    .unwrap();
    let mut declaration = builder.define_type("Counter").unwrap();
    declaration.type_parameter("T").unwrap();
    declaration
        .native_storage(NativeStorage::new(move |context| {
            // SAFETY: Inputs are scalars; no collection occurs before the returned
            // payload publishes its array edge through the ordinary scanner.
            let values = unsafe {
                context.allocate_array_transport(
                    context.resolve_type(Type::i32().abi())?,
                    vec![Value::I32(42)],
                )
            }?;
            Ok(TracedCounter {
                values,
                contract: None,
                dropped: factory_dropped.clone(),
            })
        }))
        .unwrap();
    let counter = declaration.finish().unwrap();
    let ty = counter.apply([Type::i32()]).unwrap();
    let new = builder
        .define_function(FunctionDecl::new("new").returns(ty.clone()))
        .unwrap();
    builder
        .bind(
            new,
            // SAFETY: The native argument frame and local working roots protect inputs; results are
            // decoded, rooted or returned before another callback.
            unsafe {
                NativeBinding::new_transport(vec![], counter.codec(), |context| {
                    context.allocate_result_transport()
                })
            },
        )
        .unwrap();
    let read = builder
        .define_function(
            FunctionDecl::new("read")
                .parameter("counter", ty)
                .returns(Type::i32()),
        )
        .unwrap();
    builder
        .bind(read, unsafe {
            NativeBinding::new_transport(
                vec![counter.codec()],
                Codec::Exact(Type::i32()),
                |context| {
                    context.with_payload::<TracedCounter, _>(0, |payload| {
                        // Collection and execution cannot invalidate this scoped Rust borrow.
                        assert_eq!(
                            context.allocate_result_transport().unwrap_err().message(),
                            "module validation failed: script reentry during heap storage access"
                        );
                        let Value::Array(id) = payload.values.value_transport() else {
                            panic!("counter payload");
                        };
                        context
                            .heap()
                            .array_get_transport(id, 0)
                            .ok_or_else(|| RuntimeError::module_validation("missing traced child"))
                    })
                },
            )
        })
        .unwrap();
    let module = builder.finish().unwrap();
    let (vm, loaded) = compile(
        "use example::objects::{Counter, new, read}; fn main() -> Counter<i32> { new() } pub fn answer() -> i32 { read(new()) }",
        Some(&module),
    );
    let _loaded_program = loaded;
    let loaded = vm.runtime().select_program(&_loaded_program).unwrap();
    let source_root = vm
        .execute(&loaded, ProgramEntry::Main)
        .unwrap()
        .return_value;
    // SAFETY: The source remains retained until the independent data root is published.
    let value = unsafe { source_root.value_transport(vm.runtime().gc()) }
        .expect("retained execution result");
    let rooted = unsafe { vm.runtime().gc().root_value_transport(value) }.unwrap();
    drop(source_root);
    vm.runtime().collect_garbage().unwrap();
    assert_eq!(dropped.load(Ordering::SeqCst), 0);
    assert_eq!(
        vm.execute(&loaded, "answer")
            .unwrap()
            .return_value
            .inspect(vm.runtime(), |view| view.as_i32().map(Value::I32))
            .expect("retained execution result"),
        Value::I32(42)
    );
    drop(rooted);
    vm.runtime().collect_garbage().unwrap();
    assert_eq!(dropped.load(Ordering::SeqCst), 2);
}

#[test]
fn default_array_literals_repeats_and_empty_arrays_use_contiguous_scalar_storage() {
    let language = StandardDeclarations::default();
    let mut builder = ModuleBuilder::new(
        "example::arrays",
        &language.catalog().expect("explicit standard providers"),
    )
    .unwrap();
    let array = language.vec(Type::i32()).unwrap();
    let probe = builder
        .define_function(
            FunctionDecl::new("probe")
                .parameter("values", array)
                .returns(Type::i32()),
        )
        .unwrap();
    builder
        .bind(
            probe,
            |cx: &mut NativeContext<'_>, (values,): (ScriptVec<i32>,)| -> NativeResult<i32> {
                assert!(values.with_slice::<u32, _>(cx, |_| Ok(())).is_err());
                values.with_slice_mut::<i32, _>(cx, |values| {
                    values.reverse();
                    Ok(())
                })?;
                values.with_slice::<i32, _>(cx, |values| Ok(values.iter().copied().sum()))
            },
        )
        .unwrap();
    let module = builder.finish().unwrap();
    let (vm, loaded) = compile(
        r#"
        use example::arrays::probe;
        fn main() -> i32 {
            val empty: Vec<i32> = Vec::from([]);
            val values = Vec::from([20, 22]);
            val repeated = Vec::from([7; 6]);
            if probe(empty) != 0 || probe(repeated) != 42 { return -1; }
            val alias = values;
            if probe(values) == 42 && alias[0] == 22 { 42 } else { -2 }
        }
    "#,
        Some(&module),
    );
    let _loaded_program = loaded;
    let loaded = vm.runtime().select_program(&_loaded_program).unwrap();
    assert_eq!(
        vm.execute(&loaded, ProgramEntry::Main)
            .unwrap()
            .return_value
            .inspect(vm.runtime(), |view| view.as_i32().map(Value::I32))
            .expect("retained execution result"),
        Value::I32(42)
    );
}

fn rooted_sequence_len(
    cx: &mut NativeContext<'_>,
    (values,): (ScriptVec<i32>,),
) -> NativeResult<usize> {
    let length = values.len(cx)?;
    cx.collect_garbage()?;
    assert_eq!(values.len(cx)?, length);
    values.with_slice::<i32, _>(cx, |_| {
        let error = cx.collect_garbage().unwrap_err();
        assert_eq!(error.kind(), RuntimeErrorKind::ModuleValidation);
        assert!(error.message().contains("heap storage access"));
        Ok(())
    })?;
    Ok(length)
}

fn transform_sequence(
    cx: &mut NativeContext<'_>,
    (increment, values, scale): (i32, ScriptVec<i32>, i32),
) -> NativeResult<i32> {
    values.with_slice_mut::<i32, _>(cx, |values| {
        for value in values.iter_mut() {
            *value = (*value + increment) * scale;
        }
        Ok(values.iter().copied().sum())
    })
}

#[test]
fn ordinary_sequence_parameters_borrow_roots_and_preserve_mutation_guards() {
    let language = StandardDeclarations::default();
    let mut builder = ModuleBuilder::new(
        "example::views",
        &language.catalog().expect("explicit standard providers"),
    )
    .unwrap();
    let len = builder
        .define_function(
            FunctionDecl::new("len")
                .parameter("values", language.vec(Type::i32()).unwrap())
                .returns(Type::u64()),
        )
        .unwrap();
    builder.bind(len, rooted_sequence_len).unwrap();
    let transform = builder
        .define_function(
            FunctionDecl::new("transform")
                .parameter("increment", Type::i32())
                .parameter("values", language.vec(Type::i32()).unwrap())
                .parameter("scale", Type::i32())
                .returns(Type::i32()),
        )
        .unwrap();
    builder.bind(transform, transform_sequence).unwrap();
    let module = builder.finish().unwrap();
    let (vm, loaded) = compile(
        r#"
        use example::views::{len, transform};
        fn main() -> i32 {
            val values = Vec::from([9, 10]);
            val alias = values;
            if len(values) != 2 || len(Vec::new()) != 0 { return -1; }
            if transform(1, values, 2) != 42 { return -2; }
            if alias[0] == 20 && alias[1] == 22 { 42 } else { -3 }
        }
        pub fn guarded() -> i32 {
            val values = Vec::from([1, 2]);
            for value in values { transform(1, values, value); }
            0
        }
        pub fn healthy() -> i32 { transform(1, Vec::from([9, 10]), 2) }
    "#,
        Some(&module),
    );
    let _loaded_program = loaded;
    let loaded = vm.runtime().select_program(&_loaded_program).unwrap();
    assert_eq!(
        vm.execute(&loaded, ProgramEntry::Main)
            .unwrap()
            .return_value
            .inspect(vm.runtime(), |view| view.as_i32().map(Value::I32))
            .expect("retained execution result"),
        Value::I32(42)
    );
    assert!(vm.execute(&loaded, "guarded").is_err());
    assert_eq!(
        vm.execute(&loaded, "healthy")
            .unwrap()
            .return_value
            .inspect(vm.runtime(), |view| view.as_i32().map(Value::I32))
            .expect("retained execution result"),
        Value::I32(42)
    );
}

#[test]
fn typed_vector_edits_validate_before_committing_and_trace_reference_elements() {
    let (vm, loaded) = compile(
        "pub struct Node { pub val value: i32 } pub fn values() -> Vec<i32> { Vec::from([1, 2, 3, 4]) } pub fn nodes() -> Vec<Node> { Vec::from([Node { value: 42 }]) }",
        None,
    );
    let program = loaded;
    let loaded = vm.runtime().select_program(&program).unwrap();
    let vector: ScriptVec<i32> = vm.execute_typed(&loaded, "values", ()).unwrap();
    let mut cx = vm.context(&loaded).unwrap();
    assert!(
        vector
            .with_slice_mut::<bool, _>(&mut cx, |_| Ok(()))
            .is_err()
    );
    assert_eq!(
        vector
            .with_slice::<i32, _>(&cx, |v| Ok(v.to_vec()))
            .unwrap(),
        [1, 2, 3, 4]
    );
    vector
        .with_slice_mut::<i32, _>(&mut cx, |v| {
            v.copy_within(0..3, 1);
            Ok(())
        })
        .unwrap();
    assert_eq!(
        vector
            .with_slice::<i32, _>(&cx, |v| Ok(v.to_vec()))
            .unwrap(),
        [1, 1, 2, 3]
    );
    vector.set(&mut cx, 0, 20).unwrap();
    vector.reverse(&mut cx).unwrap();
    vector.truncate(&mut cx, 2).unwrap();
    vector.push(&mut cx, 7).unwrap();
    assert_eq!(
        vector
            .with_slice::<i32, _>(&cx, |v| Ok(v.to_vec()))
            .unwrap(),
        [3, 2, 7]
    );
    vector
        .with_slice_mut::<i32, _>(&mut cx, |v| {
            v.fill(7);
            Ok(())
        })
        .unwrap();
    vector.set(&mut cx, 0, 2).unwrap();
    let mut index = 0;
    vector
        .retain(&mut cx, |_, _| {
            index += 1;
            Ok(index != 2)
        })
        .unwrap();
    assert_eq!(
        vector
            .with_slice::<i32, _>(&cx, |v| Ok(v.to_vec()))
            .unwrap(),
        [2, 7]
    );
    assert_eq!(vector.remove(&mut cx, 0).unwrap(), Some(2));
    assert_eq!(vector.get(&mut cx, 0).unwrap(), Some(7));
    drop(vector);
    let nodes: ScriptVec<Object> = vm.execute_typed(&loaded, "nodes", ()).unwrap();
    let node = vm
        .runtime()
        .bind_type(&program.root(), "Node", &[])
        .unwrap();
    let field = vm.runtime().bind_field::<i32>(&node, "value").unwrap();
    vm.runtime().collect_garbage().unwrap();
    assert_eq!(
        nodes
            .get(&mut cx, 0)
            .unwrap()
            .unwrap()
            .get(&mut cx, &field)
            .unwrap(),
        42
    );
}

#[test]
fn every_scalar_layout_is_selected_from_the_declared_array_element() {
    use kagari_types::scalar::BuiltinType;
    let language = StandardDeclarations::default();
    let mut builder = ModuleBuilder::new(
        "example::scalar_arrays",
        &language.catalog().expect("explicit standard providers"),
    )
    .unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let mut expressions = Vec::new();
    macro_rules! check {
        ($kind:ident, $rust:ty, $expected:expr, $literal:literal) => {{
            let name = concat!("check_", stringify!($kind));
            let function = builder
                .define_function(
                    FunctionDecl::new(name)
                        .parameter(
                            "values",
                            language.vec(Type::scalar(BuiltinType::$kind)).unwrap(),
                        )
                        .returns(Type::bool()),
                )
                .unwrap();
            let count = calls.clone();
            builder
                .bind(
                    function,
                    move |cx: &mut NativeContext<'_>,
                          (values,): (ScriptVec<$rust>,)|
                          -> NativeResult<bool> {
                        values.with_slice::<$rust, _>(cx, |values| {
                            assert!(values.len() <= 2);
                            assert!(values.iter().all(|value| *value == $expected));
                            count.fetch_add(1, Ordering::SeqCst);
                            Ok(true)
                        })
                    },
                )
                .unwrap();
            expressions.push(format!(
                "{name}(Vec::new()) && {name}(Vec::from([{0}])) && {name}(Vec::from([{0}; 2]))",
                $literal
            ));
        }};
    }

    check!(Unit, (), (), "()");
    check!(Bool, bool, true, "true");
    check!(I8, i8, 1, "1 as i8");
    check!(I16, i16, 1, "1 as i16");
    check!(I32, i32, 1, "1");
    check!(I64, i64, i64::MIN, "-9223372036854775808i64");
    check!(U8, u8, 1, "1 as u8");
    check!(U16, u16, 1, "1 as u16");
    check!(U32, u32, 1, "1 as u32");
    check!(U64, u64, u64::MAX, "18446744073709551615u64");
    check!(F32, f32, 1.0, "1.0 as f32");
    check!(F64, f64, 1.0, "1.0");
    let module = builder.finish().unwrap();
    let statements = expressions
        .iter()
        .map(|expression| format!("if !({expression}) {{ return false; }}"))
        .collect::<Vec<_>>()
        .join(" ");
    let source =
        format!("use example::scalar_arrays::*; fn main() -> bool {{ {statements} true }}");
    let (vm, loaded) = compile(&source, Some(&module));
    let _loaded_program = loaded;
    let loaded = vm.runtime().select_program(&_loaded_program).unwrap();
    assert_eq!(
        vm.execute(&loaded, ProgramEntry::Main)
            .unwrap()
            .return_value
            .inspect(vm.runtime(), |view| view.as_bool().map(Value::Bool))
            .expect("retained execution result"),
        Value::Bool(true)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 36);
}

#[test]
fn hash_callbacks_can_collect_and_trap_without_losing_keys_or_lookup_guards() {
    let mut builder = ModuleBuilder::new(
        "example::hash_gc",
        &StandardDeclarations::default()
            .catalog()
            .expect("explicit standard providers"),
    )
    .unwrap();
    let collect = builder
        .define_function(FunctionDecl::new("collect").returns(Type::i64()))
        .unwrap();
    let count = Arc::new(AtomicUsize::new(0));
    let capture = count.clone();
    builder
        .bind(
            collect,
            move |cx: &mut CallContext<'_>| -> NativeResult<i64> {
                cx.collect_garbage()?;
                capture.fetch_add(1, Ordering::SeqCst);
                Ok(7)
            },
        )
        .unwrap();
    let module = builder.finish().unwrap();
    let (vm, loaded) = compile(
        include_str!("../../tests/fixtures/native/hash-gc-and-trap.kgr"),
        Some(&module),
    );
    let _loaded_program = loaded;
    let loaded = vm.runtime().select_program(&_loaded_program).unwrap();
    assert!(vm.execute(&loaded, "failing").is_err());
    vm.runtime().collect_garbage().unwrap();
    assert_eq!(
        vm.execute(&loaded, "healthy")
            .unwrap()
            .return_value
            .inspect(vm.runtime(), |view| view.as_i32().map(Value::I32))
            .expect("retained execution result"),
        Value::I32(42)
    );
    assert!(count.load(Ordering::SeqCst) > 10);
    let collected = vm.runtime().collect_garbage().unwrap();
    assert_eq!(collected.live_objects, 0);
}

#[test]
fn empty_hash_containers_reject_wrong_types_and_keep_the_selected_key_protocol() {
    let (vm, loaded) = compile(
        r#"use std::collections::{HashMap, HashSet};
use std::hash::{Hash};

        struct Key { val value: i32 }
        impl PartialEq for Key { fn eq(self, other: Key) -> bool { self.value == other.value } }
        impl Eq for Key {}
        impl Hash for Key { fn hash(self) -> i64 { 7 } }
        pub fn empty_map() -> HashMap<i32, i32> { HashMap::new() }
        pub fn empty_set() -> HashSet<i32> { HashSet::new() }
        pub fn custom() -> HashMap<Key, i32> { HashMap::new() }
        pub fn key() -> Key { Key { value: 1 } }
    "#,
        None,
    );
    let _loaded_program = loaded;
    let loaded = vm.runtime().select_program(&_loaded_program).unwrap();
    let map: ScriptMap<ScriptValue, ScriptValue> =
        vm.execute_typed(&loaded, "empty_map", ()).unwrap();
    let set: ScriptSet<ScriptValue> = vm.execute_typed(&loaded, "empty_set", ()).unwrap();
    let custom: ScriptMap<Object, i32> = vm.execute_typed(&loaded, "custom", ()).unwrap();
    let key: Object = vm.execute_typed(&loaded, "key", ()).unwrap();
    let mut conversion = ConversionContext::new(vm.runtime(), &loaded).unwrap();
    let wrong_type = conversion.type_for::<bool>().unwrap();
    let wrong = conversion.encode_value(&wrong_type, true).unwrap();
    let one_type = conversion.type_for::<i32>().unwrap();
    let one = conversion.encode_value(&one_type, 1_i32).unwrap();
    let mut cx = vm.context(&loaded).unwrap();
    assert!(map.insert(&mut cx, wrong.clone(), one.clone()).is_err());
    assert!(map.insert(&mut cx, one, wrong.clone()).is_err());
    assert!(set.insert(&mut cx, wrong).is_err());
    assert_eq!(map.len(&cx).unwrap(), 0);
    assert_eq!(set.len(&cx).unwrap(), 0);
    custom.insert(&mut cx, key.clone(), 42).unwrap();
    assert_eq!(custom.get(&mut cx, key.clone()).unwrap(), Some(42));
    custom.clear(&mut cx).unwrap();
    assert_eq!(custom.len(&cx).unwrap(), 0);
    // Clearing must preserve the originally selected custom Hash/Eq protocol.
    custom.insert(&mut cx, key.clone(), 43).unwrap();
    assert_eq!(custom.get(&mut cx, key).unwrap(), Some(43));
}

#[test]
fn native_cursor_keeps_its_source_alive_and_shares_position_across_calls() {
    let language = StandardDeclarations::default();
    let mut builder = ModuleBuilder::new(
        "example::cursors",
        &language.catalog().expect("explicit standard providers"),
    )
    .unwrap();
    let hold = builder
        .define_function(
            FunctionDecl::new("hold").returns(language.collection_cursor(Type::i32()).unwrap()),
        )
        .unwrap();
    let slot = Arc::new(Mutex::new(None::<kagari_runtime::gc::roots::RootedValue>));
    let capture = slot.clone();
    builder
        .bind(hold, unsafe {
            NativeBinding::new_transport(vec![], Codec::Iterator, move |cx| {
                capture
                    .lock()
                    .unwrap()
                    .as_ref()
                    .and_then(|root| root.value_transport(cx.heap()))
                    .ok_or_else(|| RuntimeError::module_validation("missing retained cursor"))
            })
        })
        .unwrap();
    let module = builder.finish().unwrap();
    let (vm, loaded) = compile(
        r#"use std::iter::{CollectionCursor};

        use example::cursors::hold;
        pub fn cursor() -> CollectionCursor<i32> { [20, 22].iter() }
        pub fn first() -> i32 { match hold().next() { Some(value) => value, None => -1 } }
        pub fn rest() -> i32 { var total = 0; for value in hold() { total = total + value; } total }
    "#,
        Some(&module),
    );
    let _loaded_program = loaded;
    let loaded = vm.runtime().select_program(&_loaded_program).unwrap();
    let value_root = vm.execute(&loaded, "cursor").unwrap().return_value;
    // SAFETY: value_root protects this value through its uses and traced publication.
    let value = unsafe { value_root.value_transport(vm.runtime().gc()) }
        .expect("retained execution result");
    *slot.lock().unwrap() = Some(unsafe { vm.runtime().gc().root_value_transport(value) }.unwrap());
    drop(value_root);
    vm.runtime().collect_garbage().unwrap();
    assert_eq!(
        vm.execute(&loaded, "first")
            .unwrap()
            .return_value
            .inspect(vm.runtime(), |view| view.as_i32().map(Value::I32))
            .expect("retained execution result"),
        Value::I32(20)
    );
    vm.runtime().collect_garbage().unwrap();
    assert_eq!(
        vm.execute(&loaded, "rest")
            .unwrap()
            .return_value
            .inspect(vm.runtime(), |view| view.as_i32().map(Value::I32))
            .expect("retained execution result"),
        Value::I32(22)
    );
    assert_eq!(
        vm.execute(&loaded, "rest")
            .unwrap()
            .return_value
            .inspect(vm.runtime(), |view| view.as_i32().map(Value::I32))
            .expect("retained execution result"),
        Value::I32(0)
    );
    *slot.lock().unwrap() = None;
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
}

#[test]
fn native_constructor_supplies_a_traced_payload_without_a_default_factory() {
    let language = StandardDeclarations::default();
    let mut builder = ModuleBuilder::new(
        "example::provided",
        &language.catalog().expect("explicit standard providers"),
    )
    .unwrap();
    let mut declaration = builder.define_type("Counter").unwrap();
    declaration
        .native_storage(NativeStorage::payload::<TracedCounter>())
        .unwrap();
    let counter = declaration.finish().unwrap();
    let ty = counter.apply([]).unwrap();
    let dropped = Arc::new(AtomicUsize::new(0));
    let new = builder
        .define_function(
            FunctionDecl::new("new")
                .parameter("values", language.vec(Type::i32()).unwrap())
                .returns(ty.clone()),
        )
        .unwrap();
    let capture = dropped.clone();
    builder
        .bind(new, unsafe {
            NativeBinding::new_transport(vec![Codec::Value], counter.codec(), move |cx| {
                let payload = TracedCounter {
                    // SAFETY: The call retains this input until the payload scanner publishes its edge.
                    values: cx.native_context()?.conversion().store_transport(
                        &cx.argument_type_argument(0)?,
                        cx.argument_transport(0)?,
                    )?,
                    contract: Some(StorageType::prepare_scoped(
                        cx.argument_type_argument(0)?,
                        cx.owner(),
                    )?),
                    dropped: capture.clone(),
                };
                // SAFETY: The native argument frame and local working roots protect inputs; results
                // are decoded, rooted or returned before another callback.
                cx.allocate_result_payload_transport(&[], |_, _| Ok(payload))
            })
        })
        .unwrap();
    let wrong = builder
        .define_function(FunctionDecl::new("missing_payload").returns(ty.clone()))
        .unwrap();
    builder
        .bind(
            wrong,
            // SAFETY: The native argument frame and local working roots protect inputs; results are
            // decoded, rooted or returned before another callback.
            unsafe {
                NativeBinding::new_transport(vec![], counter.codec(), |cx| {
                    cx.allocate_result_transport()
                })
            },
        )
        .unwrap();
    let read = builder
        .define_function(
            FunctionDecl::new("read")
                .parameter("counter", ty)
                .returns(Type::i32()),
        )
        .unwrap();
    builder
        .bind(read, unsafe {
            NativeBinding::new_transport(vec![counter.codec()], Codec::Exact(Type::i32()), |cx| {
                let mut native = cx.native_context()?;
                // SAFETY: CallContext retains the complete input argument snapshot.
                let object: Opaque<TracedCounter> = native
                    .conversion()
                    .decode_transport(&cx.argument_type_argument(0)?, &cx.argument_transport(0)?)?;
                let values = object.read(&native, None, |payload, access| {
                    access.retain(
                        payload.contract.as_ref().expect("provided child contract"),
                        &payload.values,
                    )
                })?;
                let values = values.decode::<ScriptVec<i32>>(&mut native)?;
                values
                    .get(&mut native, 0)?
                    .map(Value::I32)
                    .ok_or_else(|| RuntimeError::module_validation("counter element"))
            })
        })
        .unwrap();
    let module = builder.finish().unwrap();
    let (vm, loaded) = compile(
        r#"
        use example::provided::{Counter, new, read, missing_payload};
        fn main() -> Counter { new(Vec::from([42])) }
        pub fn healthy() -> i32 { read(new(Vec::from([42]))) }
        pub fn invalid() -> Counter { missing_payload() }
    "#,
        Some(&module),
    );
    let _loaded_program = loaded;
    let loaded = vm.runtime().select_program(&_loaded_program).unwrap();
    let error = vm.execute(&loaded, "invalid").unwrap_err();
    let VmError::RuntimeError(error) = error.cause() else {
        panic!("expected storage allocation error");
    };
    assert!(error.message().contains("explicitly supplied payload"));
    let source_root = vm
        .execute(&loaded, ProgramEntry::Main)
        .unwrap()
        .return_value;
    // SAFETY: The source remains retained until the independent data root is published.
    let value = unsafe { source_root.value_transport(vm.runtime().gc()) }
        .expect("retained execution result");
    let root = unsafe { vm.runtime().gc().root_value_transport(value) }.unwrap();
    drop(source_root);
    vm.runtime().collect_garbage().unwrap();
    assert_eq!(dropped.load(Ordering::SeqCst), 0);
    assert_eq!(
        vm.execute(&loaded, "healthy")
            .unwrap()
            .return_value
            .inspect(vm.runtime(), |view| view.as_i32().map(Value::I32))
            .expect("retained execution result"),
        Value::I32(42)
    );
    drop(root);
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
    assert_eq!(dropped.load(Ordering::SeqCst), 2);
}

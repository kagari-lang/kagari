use crate::{error::VmError, vm::Vm};
use kagari_bytecode::program::BytecodeProgram;
use kagari_common::identity::metadata::DefinitionMetadata;
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_hir::{analysis::AnalysisDatabase, compile_input::program::CheckedCompilation};

use kagari_runtime::{
    Runtime,
    error::RuntimeError,
    module::{InstalledProgram, entry::ProgramEntry},
    native::{
        binding::{Codec, NativeBinding, NativeResult},
        builder::ModuleBuilder,
        context::CallContext,
        declarations::FunctionDecl,
        module::NativeModule,
        scalar::NativeScalar,
        storage::{NativePayload, NativeStorage},
        types::Type,
    },
    resource::HeapCharge,
    value::Value,
};
use kagari_source::source_database::{SourceDatabase, SourceLayer};

use std::sync::atomic::{AtomicUsize, Ordering};
use std::{ops::Bound, sync::Arc};
use {kagari_stdlib as foundation, kagari_stdlib::declarations::StandardDeclarations};

mod native_boundary_artifacts;
mod native_boundary_callbacks;
mod native_boundary_control;
mod native_boundary_conversion;
mod native_boundary_enums;
mod native_boundary_functions;
mod native_boundary_gc;
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
    let mir = lower_program_to_mir(
        &CheckedCompilation::from_program(&checked, &Default::default()).unwrap(),
        &Default::default(),
    )
    .unwrap();
    lower_program_to_bytecode(&mir, &Default::default())
        .map(|program| program.into_unverified())
        .unwrap()
}

fn compile(text: &str, module: Option<&NativeModule>) -> (Vm, InstalledProgram) {
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
fn publish_unused_version(vm: &mut Vm, active: &InstalledProgram) {
    let selected = vm.runtime().select_program(active).unwrap();
    let candidate = vm
        .runtime()
        .stage_reload_verified_program(active, None, selected.verified_program().clone())
        .unwrap();
    vm.runtime_mut().publish_staged_reload(candidate).unwrap();
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
            .value(vm.runtime().gc())
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
        .bind(
            function,
            |_cx: &mut CallContext<'_>| -> NativeResult<Value> { Ok(Value::Bool(true)) },
        )
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
            .value(vm.runtime().gc())
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
            .value(vm.runtime().gc())
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
            .value(vm.runtime().gc())
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
        .bind(
            repeat,
            NativeBinding::new(
                vec![Codec::Value, Codec::Exact(Type::i32())],
                Codec::Exact(Type::i32()),
                move |cx| {
                    let receiver = cx.argument(0)?;
                    let Value::I32(count) = cx.argument(1)? else {
                        unreachable!()
                    };
                    let target = cx.selected(&selected)?;
                    let mut value = Value::I32(0);
                    for _ in 0..count {
                        cx.poll()?;
                        value = cx.call_values(target, &[receiver, value])?;
                    }
                    Ok(value)
                },
            ),
        )
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
                .value(vm.runtime().gc())
                .expect("retained execution result"),
            Value::I32(42)
        );
    }
}

#[derive(Debug)]
struct TracedCounter {
    values: Value,
    dropped: Arc<AtomicUsize>,
}

impl NativePayload for TracedCounter {
    fn trace<'payload>(&'payload self, visit: &mut dyn FnMut(&'payload Value)) {
        visit(&self.values);
    }

    fn charge(&self) -> NativeResult<HeapCharge> {
        Ok(HeapCharge::new(1, 0))
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
            let values = context.allocate_array(
                context.resolve_type(Type::i32().abi())?,
                vec![Value::I32(42)],
            )?;
            Ok(TracedCounter {
                values,
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
            NativeBinding::new(vec![], counter.codec(), |context| context.allocate_result()),
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
        .bind(
            read,
            NativeBinding::new(
                vec![counter.codec()],
                Codec::Exact(Type::i32()),
                |context| {
                    context.with_payload::<TracedCounter, _>(0, |payload| {
                        // Collection and execution cannot invalidate this scoped Rust borrow.
                        assert!(
                            context
                                .allocate_array(context.resolve_type(Type::i32().abi())?, vec![])
                                .unwrap_err()
                                .to_string()
                                .contains("native storage access")
                        );
                        let Value::Array(id) = payload.values else {
                            panic!("counter payload");
                        };
                        context
                            .heap()
                            .array_get(id, 0)
                            .ok_or_else(|| RuntimeError::module_validation("missing traced child"))
                    })
                },
            ),
        )
        .unwrap();
    let module = builder.finish().unwrap();
    let (vm, loaded) = compile(
        "use example::objects::{Counter, new, read}; fn main() -> Counter<i32> { new() } pub fn answer() -> i32 { read(new()) }",
        Some(&module),
    );
    let _loaded_program = loaded;
    let loaded = vm.runtime().select_program(&_loaded_program).unwrap();
    let value = vm
        .execute(&loaded, ProgramEntry::Main)
        .unwrap()
        .return_value
        .value(vm.runtime().gc())
        .expect("retained execution result");
    let rooted = vm.runtime().gc().root_value(value).unwrap();
    vm.runtime().collect_garbage().unwrap();
    assert_eq!(dropped.load(Ordering::SeqCst), 0);
    assert_eq!(
        vm.execute(&loaded, "answer")
            .unwrap()
            .return_value
            .value(vm.runtime().gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
    drop(rooted);
    vm.runtime().collect_garbage().unwrap();
    assert_eq!(dropped.load(Ordering::SeqCst), 2);
}

#[test]
fn array_literals_repeats_and_empty_arrays_preserve_native_access_and_aliases() {
    let language = StandardDeclarations::default();
    let mut builder = ModuleBuilder::new(
        "example::arrays",
        &language.catalog().expect("explicit standard providers"),
    )
    .unwrap();
    let array = Type::array(Type::i32()).unwrap();
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
            NativeBinding::new(vec![Codec::Value], Codec::Exact(Type::i32()), |cx| {
                let Value::Array(id) = cx.argument(0)? else {
                    panic!("checked array argument")
                };
                let mut values = cx.heap().array_snapshot(id).unwrap();
                values.reverse();
                let mut total = 0;
                for (index, value) in values.into_iter().enumerate() {
                    cx.heap().array_set(id, index, value)?;
                    let Value::I32(value) = value else {
                        panic!("checked element")
                    };
                    total += value;
                }
                Ok(Value::I32(total))
            }),
        )
        .unwrap();
    let module = builder.finish().unwrap();
    let (vm, loaded) = compile(
        r#"
        use example::arrays::probe;
        fn main() -> i32 {
            val empty: [i32] = [];
            val values = [20, 22];
            val repeated = [7; 6];
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
            .value(vm.runtime().gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
}

#[test]
fn typed_array_bulk_changes_validate_before_committing_and_trace_reference_elements() {
    let (vm, loaded) = compile(
        "struct Node { val value: i32 } pub fn values() -> [i32] { [1, 2, 3, 4] } pub fn nodes() -> [Node] { [Node { value: 42 }] }",
        None,
    );
    let _loaded_program = loaded;
    let loaded = vm.runtime().select_program(&_loaded_program).unwrap();
    let value = vm
        .execute(&loaded, "values")
        .unwrap()
        .return_value
        .value(vm.runtime().gc())
        .expect("retained execution result");
    let rooted = vm.runtime().root_value(value).unwrap();
    let Value::Array(id) = value else {
        panic!("array result");
    };
    let heap = vm.runtime().gc();
    assert!(heap.array_fill(id, Value::Bool(true)).is_err());
    assert_eq!(
        heap.array_snapshot(id).unwrap(),
        vec![Value::I32(1), Value::I32(2), Value::I32(3), Value::I32(4)]
    );
    heap.array_copy_within(id, Bound::Included(0), Bound::Excluded(3), 1)
        .unwrap();
    assert_eq!(
        heap.array_snapshot(id).unwrap(),
        vec![Value::I32(1), Value::I32(1), Value::I32(2), Value::I32(3)]
    );
    let wrong = vm
        .runtime()
        .alloc_array(
            &loaded,
            kagari_types::ty::Ty::Builtin(kagari_types::scalar::BuiltinType::Bool),
            vec![Value::Bool(true); 4],
        )
        .unwrap();
    let wrong_root = vm.runtime().root_value(Value::Array(wrong)).unwrap();
    assert!(heap.array_copy_from(id, wrong).is_err());
    assert_eq!(
        heap.array_snapshot(id).unwrap(),
        vec![Value::I32(1), Value::I32(1), Value::I32(2), Value::I32(3)]
    );
    heap.array_fill(id, Value::I32(7)).unwrap();
    heap.array_set(id, 0, Value::I32(2)).unwrap();
    assert_eq!(
        heap.array_snapshot(id).unwrap(),
        vec![Value::I32(2), Value::I32(7), Value::I32(7), Value::I32(7)]
    );
    drop(wrong_root);
    drop(rooted);
    let nodes = vm
        .execute(&loaded, "nodes")
        .unwrap()
        .return_value
        .value(vm.runtime().gc())
        .expect("retained execution result");
    let root = vm.runtime().root_value(nodes).unwrap();
    vm.runtime().collect_garbage().unwrap();
    let Value::Array(id) = nodes else {
        panic!("string array result");
    };
    let Some(Value::Struct(child)) = vm.runtime().gc().array_get(id, 0) else {
        panic!("traced node");
    };
    let (_, fields) = vm.runtime().gc().struct_snapshot(child).unwrap();
    assert_eq!(fields[0].value, Value::I32(42));
    drop(root);
}

#[test]
fn every_scalar_array_preserves_its_declared_element_and_values() {
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
                            Type::array(Type::scalar(BuiltinType::$kind)).unwrap(),
                        )
                        .returns(Type::bool()),
                )
                .unwrap();
            let count = calls.clone();
            builder
                .bind(
                    function,
                    NativeBinding::new(vec![Codec::Value], Codec::Exact(Type::bool()), move |cx| {
                        let Value::Array(id) = cx.argument(0)? else {
                            panic!("checked array")
                        };
                        assert_eq!(
                            cx.argument_type_parameter(0, 0)?.ty(),
                            &kagari_types::ty::Ty::Builtin(BuiltinType::$kind)
                        );
                        let values = cx.heap().array_snapshot(id).unwrap();
                        assert!(values.len() <= 2);
                        for value in values {
                            assert_eq!(<$rust as NativeScalar>::decode(value)?, $expected);
                        }
                        count.fetch_add(1, Ordering::SeqCst);
                        Ok(Value::Bool(true))
                    }),
                )
                .unwrap();
            expressions.push(format!(
                "{name}([]) && {name}([{0}]) && {name}([{0}; 2])",
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
            .value(vm.runtime().gc())
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
            .value(vm.runtime().gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
    assert!(count.load(Ordering::SeqCst) > 10);
    let collected = vm.runtime().collect_garbage().unwrap();
    assert_eq!(collected.live_objects, 0);
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
                .parameter("values", Type::array(Type::i32()).unwrap())
                .returns(ty.clone()),
        )
        .unwrap();
    let capture = dropped.clone();
    builder
        .bind(
            new,
            NativeBinding::new(vec![Codec::Value], counter.codec(), move |cx| {
                cx.allocate_result_payload(
                    TracedCounter {
                        values: cx.argument(0)?,
                        dropped: capture.clone(),
                    },
                    &[],
                )
            }),
        )
        .unwrap();
    let wrong = builder
        .define_function(FunctionDecl::new("missing_payload").returns(ty.clone()))
        .unwrap();
    builder
        .bind(
            wrong,
            NativeBinding::new(vec![], counter.codec(), |cx| cx.allocate_result()),
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
        .bind(
            read,
            NativeBinding::new(vec![counter.codec()], Codec::Exact(Type::i32()), |cx| {
                cx.with_payload::<TracedCounter, _>(0, |payload| {
                    let Value::Array(id) = payload.values else {
                        return Err(RuntimeError::module_validation("counter child"));
                    };
                    cx.heap()
                        .array_get(id, 0)
                        .ok_or_else(|| RuntimeError::module_validation("counter element"))
                })
            }),
        )
        .unwrap();
    let module = builder.finish().unwrap();
    let (vm, loaded) = compile(
        r#"
        use example::provided::{Counter, new, read, missing_payload};
        fn main() -> Counter { new([42]) }
        pub fn healthy() -> i32 { read(new([42])) }
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
    assert!(error.to_string().contains("explicitly supplied payload"));
    let value = vm
        .execute(&loaded, ProgramEntry::Main)
        .unwrap()
        .return_value
        .value(vm.runtime().gc())
        .expect("retained execution result");
    let root = vm.runtime().root_value(value).unwrap();
    vm.runtime().collect_garbage().unwrap();
    assert_eq!(dropped.load(Ordering::SeqCst), 0);
    assert_eq!(
        vm.execute(&loaded, "healthy")
            .unwrap()
            .return_value
            .value(vm.runtime().gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
    drop(root);
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
    assert_eq!(dropped.load(Ordering::SeqCst), 2);
}

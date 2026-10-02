mod native_boundary_artifacts;
mod native_boundary_callbacks;
mod native_boundary_control;
mod native_boundary_gc;
mod native_boundary_host;
mod native_boundary_interfaces;
mod native_boundary_resources;
mod native_boundary_sessions;
mod native_boundary_storage;
use kagari_bytecode::program::BytecodeProgram;
use kagari_common::source_database::{SourceDatabase, SourceLayer};
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_hir::analysis::AnalysisDatabase;
use kagari_runtime::gc::mutations::PreparedCollectionCommit;
use kagari_runtime::{
    Runtime,
    error::RuntimeError,
    native::{
        binding::{Codec, NativeBinding, NativeResult},
        builder::ModuleBuilder,
        context::CallContext,
        declarations::{FunctionDecl, MethodDecl},
        language::LanguageContracts,
        module::NativeModule,
        storage::{NativePayload, NativeStorage},
        types::Type,
        views::{SequenceHandle, SequenceMutHandle},
    },
    value::Value,
};
use kagari_vm::{error::VmError, vm::Vm};
use std::ops::Bound;
use std::{cell::Cell, rc::Rc};

fn compile_program(text: &str, module: Option<&NativeModule>) -> BytecodeProgram {
    let mut sources = SourceDatabase::default();
    let root = sources
        .set("main.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut analysis = AnalysisDatabase::default();
    analysis.set_native_modules(
        module
            .into_iter()
            .map(|module| module.declaration().clone())
            .collect(),
    );
    let snapshot = analysis
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let checked = snapshot.check_program(root, &Default::default()).unwrap();
    let mir = lower_program_to_mir(&checked, &Default::default()).unwrap();
    lower_program_to_bytecode(&mir).unwrap()
}

fn compile(
    text: &str,
    module: Option<&NativeModule>,
) -> (Vm, kagari_runtime::module::LoadedModule) {
    let program = compile_program(text, module);
    let mut runtime = Runtime::default();
    if let Some(module) = module {
        module.install(&mut runtime).unwrap();
    }
    let loaded = runtime.load_program("boundary", program).unwrap();
    (Vm::new(runtime), loaded)
}

#[test]
fn scalar_function_executes_through_the_checked_registration() {
    let mut module = ModuleBuilder::new("example::math", &LanguageContracts::default());
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
    let (mut vm, loaded) = compile(
        "use example::math::add; fn main() -> i32 { add(20, 22) }",
        Some(&module),
    );
    assert_eq!(
        vm.execute(&loaded, "main").unwrap().return_value,
        Value::I32(42)
    );
}

#[test]
fn raw_result_contract_failure_releases_the_execution_scope() {
    let mut module = ModuleBuilder::new("example::bad", &LanguageContracts::default());
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
    let (mut vm, loaded) = compile(
        "use example::bad::broken; fn main() -> i32 { broken() } fn healthy() -> i32 { 42 }",
        Some(&module),
    );
    assert!(vm.execute(&loaded, "main").is_err());
    assert_eq!(
        vm.execute(&loaded, "healthy").unwrap().return_value,
        Value::I32(42)
    );
}

#[test]
fn foundation_defaults_execute_without_optional_modules() {
    let (mut vm, loaded) = compile(
        r#"
        fn main() -> i32 {
            val values = ArrayList::new();
            values.push(20); values.push(22);
            val map = HashMap::new(); map.insert(1, values[0]); map.insert(2, values[1]);
            val set = HashSet::new(); set.insert(1); set.insert(1);
            if values.len() == (2 as usize) && map.len() == (2 as usize) && set.len() == (1 as usize) && map.contains_key(2) {
                var total = 0; for value in values { total = total + value; } total
            } else { 0 }
        }
    "#,
        None,
    );
    assert_eq!(
        vm.execute(&loaded, "main").unwrap().return_value,
        Value::I32(42)
    );
}

#[test]
fn custom_hash_collisions_use_selected_script_methods() {
    let (mut vm, loaded) = compile(
        r#"
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
            if map.len() == (2 as usize) && map.contains_key(second) {
                match found { Some(value) => value + 21, None => 0 }
            } else { 0 }
        }
    "#,
        None,
    );
    assert_eq!(
        vm.execute(&loaded, "main").unwrap().return_value,
        Value::I32(42)
    );
}

#[test]
fn selected_script_callbacks_return_directly_to_a_rust_loop() {
    use kagari_runtime::native::declarations::{CallableRequirement, MethodDecl};
    let mut module = ModuleBuilder::new("example::calls", &LanguageContracts::default());
    let mut step = module.define_trait("Step");
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
            function.parameter("item", item.ty());
            function.parameter("count", Type::i32());
            function.bound(item.ty(), step.apply([]));
            Ok(function.requires(CallableRequirement::method(item.ty(), step.method("step")?)))
        })
        .unwrap();
    module
        .bind_with(
            repeat,
            NativeBinding::new(
                vec![Codec::Value, Codec::Scalar(Type::i32().abi().clone())],
                Codec::Scalar(Type::i32().abi().clone()),
                move |cx| {
                    let receiver = cx.argument(0)?;
                    let Value::I32(count) = cx.argument(1)? else {
                        unreachable!()
                    };
                    let target = cx.selected(selected)?;
                    let mut value = Value::I32(0);
                    for _ in 0..count {
                        cx.poll()?;
                        value = cx.call_values(target, &[receiver.clone(), value])?;
                    }
                    Ok(value)
                },
            ),
        )
        .unwrap();
    let module = module.finish().unwrap();
    let (mut vm, loaded) = compile(
        r#"
        use example::calls::{Step, repeat};
        struct Counter { val amount: i32 }
        impl Step for Counter { fn step(self, value: i32) -> i32 { value + self.amount } }
        fn main() -> i32 { repeat(Counter { amount: 14 }, 3) }
    "#,
        Some(&module),
    );
    assert_eq!(
        vm.execute(&loaded, "main").unwrap().return_value,
        Value::I32(42)
    );
}

#[derive(Debug)]
struct TracedCounter {
    values: Value,
    dropped: Rc<Cell<usize>>,
}
impl NativePayload for TracedCounter {
    fn trace<'payload>(&'payload self, visit: &mut dyn FnMut(&'payload Value)) {
        visit(&self.values);
    }
    fn units(&self) -> usize {
        1
    }
}
impl Drop for TracedCounter {
    fn drop(&mut self) {
        self.dropped.set(self.dropped.get() + 1);
    }
}

#[test]
fn registered_nominal_payload_traces_children_and_drops_with_the_heap() {
    let dropped = Rc::new(Cell::new(0));
    let factory_dropped = dropped.clone();
    let mut builder = ModuleBuilder::new("example::objects", &LanguageContracts::default());
    let mut declaration = builder.define_type("Counter");
    declaration.type_parameter("T").unwrap();
    declaration
        .native_storage(NativeStorage::new(move |context| {
            let values =
                context.allocate_sequence(Type::i32().abi().clone(), vec![Value::I32(42)])?;
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
        .bind_with(
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
        .bind_with(
            read,
            NativeBinding::new(
                vec![counter.codec()],
                Codec::Scalar(Type::i32().abi().clone()),
                |context| {
                    context.with_payload::<TracedCounter, _>(0, |payload| {
                        // Collection and execution cannot invalidate this scoped Rust borrow.
                        assert!(
                            context
                                .allocate_sequence(Type::i32().abi().clone(), vec![])
                                .unwrap_err()
                                .message()
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
    let (mut vm, loaded) = compile(
        "use example::objects::{Counter, new, read}; fn main() -> Counter<i32> { new() } fn answer() -> i32 { read(new()) }",
        Some(&module),
    );
    let value = vm.execute(&loaded, "main").unwrap().return_value;
    let rooted = vm.runtime().gc().root_value(value).unwrap();
    vm.runtime().collect_garbage().unwrap();
    assert_eq!(dropped.get(), 0);
    assert_eq!(
        vm.execute(&loaded, "answer").unwrap().return_value,
        Value::I32(42)
    );
    drop(rooted);
    vm.runtime().collect_garbage().unwrap();
    assert_eq!(dropped.get(), 2);
}

#[test]
fn declared_sequence_layout_selects_contiguous_i32_even_when_empty() {
    let mut builder = ModuleBuilder::new("example::buffers", &LanguageContracts::default());
    let mut declaration = builder.define_type("Buffer");
    let item = declaration.type_parameter("T").unwrap();
    declaration.sequence_storage(&item).unwrap();
    let buffer = declaration.finish().unwrap();
    let ty = buffer.apply([Type::i32()]).unwrap();
    builder
        .implement(buffer.clone(), |group| {
            let receiver = group.receiver();
            group.inherent_impl(|methods| {
                let new =
                    methods.define_method(MethodDecl::static_method("new").returns(receiver))?;
                methods.bind_with(
                    new,
                    NativeBinding::new(vec![], buffer.codec(), |context| context.allocate_result()),
                )
            })
        })
        .unwrap();
    let push = builder
        .define_function(
            FunctionDecl::new("push")
                .parameter("buffer", ty.clone())
                .parameter("value", Type::i32()),
        )
        .unwrap();
    builder
        .bind_with(
            push,
            NativeBinding::new(
                vec![buffer.codec(), Codec::Scalar(Type::i32().abi().clone())],
                Codec::Scalar(Type::unit().abi().clone()),
                |context| {
                    context.sequence_push(0, context.argument(1)?)?;
                    Ok(Value::Unit)
                },
            ),
        )
        .unwrap();
    let sum = builder
        .define_function(
            FunctionDecl::new("sum")
                .parameter("buffer", ty)
                .returns(Type::i32()),
        )
        .unwrap();
    builder
        .bind(
            sum,
            |_context: &mut CallContext<'_>,
             mut buffer: SequenceMutHandle<'_>|
             -> NativeResult<i32> {
                assert!(
                    buffer
                        .read()
                        .with_slice::<u32, _>(|_| Ok(()))
                        .unwrap_err()
                        .message()
                        .contains("scalar layout")
                );
                buffer.with_slice_mut::<i32, _>(|values| {
                    values.reverse();
                    Ok(())
                })?;
                buffer
                    .read()
                    .with_slice::<i32, _>(|values| Ok(values.iter().copied().sum()))
            },
        )
        .unwrap();
    let module = builder.finish().unwrap();
    let (mut vm, loaded) = compile(
        "use example::buffers::{Buffer, push, sum}; fn main() -> i32 { val buffer: Buffer<i32> = Buffer::new(); val other: Buffer<i32> = Buffer::new(); if buffer != buffer || buffer == other { return -2; } val keys = HashMap::new(); keys.insert(buffer, 42); if !keys.contains_key(buffer) || keys.contains_key(other) { return -3; } if sum(buffer) != 0 { return -1; } push(buffer, 20); push(buffer, 22); sum(buffer) }",
        Some(&module),
    );
    assert_eq!(
        vm.execute(&loaded, "main").unwrap().return_value,
        Value::I32(42)
    );
}

#[test]
fn default_array_literals_repeats_and_empty_arrays_use_contiguous_scalar_storage() {
    let language = LanguageContracts::default();
    let mut builder = ModuleBuilder::new("example::arrays", &language);
    let array = language.array_list(Type::i32());
    let probe = builder
        .define_function(
            FunctionDecl::new("probe")
                .parameter("values", array)
                .returns(Type::i32()),
        )
        .unwrap();
    builder
        .bind_with(
            probe,
            NativeBinding::new(
                vec![Codec::Sequence],
                Codec::Scalar(Type::i32().abi().clone()),
                |cx| {
                    assert!(cx.with_sequence::<u32, _>(0, |_| Ok(())).is_err());
                    cx.with_sequence_mut::<i32, _>(0, |values| {
                        values.reverse();
                        Ok(())
                    })?;
                    cx.with_sequence::<i32, _>(0, |values| {
                        Ok(Value::I32(values.iter().copied().sum()))
                    })
                },
            ),
        )
        .unwrap();
    let module = builder.finish().unwrap();
    let (mut vm, loaded) = compile(
        r#"
        use example::arrays::probe;
        fn main() -> i32 {
            val empty: ArrayList<i32> = [];
            val values = [20, 22];
            val repeated = [7; 6];
            if probe(empty) != 0 || probe(repeated) != 42 { return -1; }
            val alias = values;
            if probe(values) == 42 && alias[0] == 22 { 42 } else { -2 }
        }
    "#,
        Some(&module),
    );
    assert_eq!(
        vm.execute(&loaded, "main").unwrap().return_value,
        Value::I32(42)
    );
}

fn rooted_sequence_len(
    cx: &mut CallContext<'_>,
    values: SequenceHandle<'_>,
) -> NativeResult<usize> {
    let length = values.len()?;
    cx.collect_garbage()?;
    assert_eq!(values.len()?, length);
    values.with_slice::<i32, _>(|_| {
        assert!(
            cx.collect_garbage()
                .unwrap_err()
                .message()
                .contains("native storage access")
        );
        Ok(())
    })?;
    Ok(length)
}

fn transform_sequence(
    _cx: &mut CallContext<'_>,
    increment: i32,
    mut values: SequenceMutHandle<'_>,
    scale: i32,
) -> NativeResult<i32> {
    values.with_slice_mut::<i32, _>(|values| {
        for value in values.iter_mut() {
            *value = (*value + increment) * scale;
        }
        Ok(values.iter().copied().sum())
    })
}

#[test]
fn ordinary_sequence_parameters_borrow_roots_and_preserve_mutation_guards() {
    let language = LanguageContracts::default();
    let mut builder = ModuleBuilder::new("example::views", &language);
    let len = builder
        .define_function(
            FunctionDecl::new("len")
                .parameter("values", language.array_list(Type::i32()))
                .returns(Type::usize()),
        )
        .unwrap();
    builder.bind(len, rooted_sequence_len).unwrap();
    let transform = builder
        .define_function(
            FunctionDecl::new("transform")
                .parameter("increment", Type::i32())
                .parameter("values", language.array_list(Type::i32()))
                .parameter("scale", Type::i32())
                .returns(Type::i32()),
        )
        .unwrap();
    builder.bind(transform, transform_sequence).unwrap();
    let module = builder.finish().unwrap();
    let (mut vm, loaded) = compile(
        r#"
        use example::views::{len, transform};
        fn main() -> i32 {
            val values = [9, 10];
            val alias = values;
            if len(values) != 2 || len([]) != 0 { return -1; }
            if transform(1, values, 2) != 42 { return -2; }
            if alias[0] == 20 && alias[1] == 22 { 42 } else { -3 }
        }
        fn guarded() -> i32 {
            val values = [1, 2];
            for value in values { transform(1, values, value); }
            0
        }
        fn healthy() -> i32 { transform(1, [9, 10], 2) }
    "#,
        Some(&module),
    );
    assert_eq!(
        vm.execute(&loaded, "main").unwrap().return_value,
        Value::I32(42)
    );
    assert!(vm.execute(&loaded, "guarded").is_err());
    assert_eq!(
        vm.execute(&loaded, "healthy").unwrap().return_value,
        Value::I32(42)
    );
}

#[test]
fn typed_array_bulk_changes_validate_before_committing_and_trace_reference_elements() {
    let (mut vm, loaded) = compile(
        "struct Node { val value: i32 } fn values() -> ArrayList<i32> { [1, 2, 3, 4] } fn nodes() -> ArrayList<Node> { [Node { value: 42 }] }",
        None,
    );
    let value = vm.execute(&loaded, "values").unwrap().return_value;
    let rooted = vm.runtime().root_value(value.clone()).unwrap();
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
    let guard = heap.begin_collection_iteration(&value).unwrap();
    assert!(heap.array_reverse(id).is_err());
    heap.array_set(id, 0, Value::I32(20)).unwrap();
    drop(guard);
    heap.array_reverse(id).unwrap();
    heap.array_truncate(id, 2).unwrap();
    heap.array_push(id, Value::I32(7)).unwrap();
    assert_eq!(
        heap.array_snapshot(id).unwrap(),
        vec![Value::I32(3), Value::I32(2), Value::I32(7)]
    );
    let wrong = vm
        .runtime()
        .alloc_array(
            &loaded,
            Type::bool().abi().clone(),
            vec![Value::Bool(true); 3],
        )
        .unwrap();
    assert!(heap.array_copy_from(id, wrong).is_err());
    assert_eq!(
        heap.array_snapshot(id).unwrap(),
        vec![Value::I32(3), Value::I32(2), Value::I32(7)]
    );
    heap.array_fill(id, Value::I32(7)).unwrap();
    heap.array_set(id, 0, Value::I32(2)).unwrap();
    let mask = vm
        .runtime()
        .alloc_array(
            &loaded,
            Type::bool().abi().clone(),
            vec![Value::Bool(true), Value::Bool(false), Value::Bool(true)],
        )
        .unwrap();
    heap.commit_prepared_collection(
        PreparedCollectionCommit::Retain,
        &[value.clone(), Value::Array(mask)],
    )
    .unwrap();
    assert_eq!(
        heap.array_snapshot(id).unwrap(),
        vec![Value::I32(2), Value::I32(7)]
    );
    let Value::Tuple(parts) = heap
        .prepare_array_removal(id, Bound::Included(0), Bound::Excluded(1))
        .unwrap()
    else {
        panic!("prepared removal");
    };
    let [Value::Array(remaining), Value::Array(removed)] = parts.as_slice() else {
        panic!("prepared range arrays");
    };
    assert_eq!(
        heap.array_snapshot(*remaining).unwrap(),
        vec![Value::I32(7)]
    );
    assert_eq!(heap.array_snapshot(*removed).unwrap(), vec![Value::I32(2)]);
    assert_eq!(
        heap.array_snapshot(id).unwrap(),
        vec![Value::I32(2), Value::I32(7)]
    );
    heap.array_extend(id, id).unwrap();
    assert_eq!(
        heap.array_snapshot(id).unwrap(),
        vec![Value::I32(2), Value::I32(7), Value::I32(2), Value::I32(7)]
    );
    drop(rooted);
    let nodes = vm.execute(&loaded, "nodes").unwrap().return_value;
    let root = vm.runtime().root_value(nodes.clone()).unwrap();
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
fn every_scalar_layout_is_selected_from_the_declared_array_element() {
    use kagari_abi::scalar::BuiltinType;
    let language = LanguageContracts::default();
    let mut builder = ModuleBuilder::new("example::scalar_arrays", &language);
    let calls = Rc::new(Cell::new(0));
    let mut expressions = Vec::new();
    macro_rules! check {
        ($kind:ident, $rust:ty, $expected:expr, $literal:literal) => {{
            let name = concat!("check_", stringify!($kind));
            let function = builder
                .define_function(
                    FunctionDecl::new(name)
                        .parameter(
                            "values",
                            language.array_list(Type::scalar(BuiltinType::$kind)),
                        )
                        .returns(Type::bool()),
                )
                .unwrap();
            let count = calls.clone();
            builder
                .bind_with(
                    function,
                    NativeBinding::new(
                        vec![Codec::Sequence],
                        Codec::Scalar(Type::bool().abi().clone()),
                        move |cx| {
                            cx.with_sequence::<$rust, _>(0, |values| {
                                assert!(values.is_empty() || values.len() == 2);
                                assert!(values.iter().all(|value| *value == $expected));
                                count.set(count.get() + 1);
                                Ok(Value::Bool(true))
                            })
                        },
                    ),
                )
                .unwrap();
            expressions.push(format!(
                "{name}([]) && {name}([{0}, {0}]) && {name}([{0}; 2])",
                $literal
            ));
        }};
    }
    check!(Unit, (), (), "()");
    check!(Bool, bool, true, "true");
    check!(I8, i8, 1, "1 as i8");
    check!(I16, i16, 1, "1 as i16");
    check!(I32, i32, 1, "1");
    check!(I64, i64, 1, "1 as i64");
    check!(ISize, isize, 1, "1 as isize");
    check!(U8, u8, 1, "1 as u8");
    check!(U16, u16, 1, "1 as u16");
    check!(U32, u32, 1, "1 as u32");
    check!(U64, u64, 1, "1 as u64");
    check!(USize, usize, 1, "1 as usize");
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
    let (mut vm, loaded) = compile(&source, Some(&module));
    assert_eq!(
        vm.execute(&loaded, "main").unwrap().return_value,
        Value::Bool(true)
    );
    assert_eq!(calls.get(), 42);
}

#[test]
fn hash_callbacks_can_collect_and_trap_without_losing_keys_or_lookup_guards() {
    let mut builder = ModuleBuilder::new("example::hash_gc", &LanguageContracts::default());
    let collect = builder
        .define_function(FunctionDecl::new("collect").returns(Type::i64()))
        .unwrap();
    let count = Rc::new(Cell::new(0));
    let capture = count.clone();
    builder
        .bind(
            collect,
            move |cx: &mut CallContext<'_>| -> NativeResult<i64> {
                cx.collect_garbage()?;
                capture.set(capture.get() + 1);
                Ok(7)
            },
        )
        .unwrap();
    let module = builder.finish().unwrap();
    let (mut vm, loaded) = compile(
        r#"
        use example::hash_gc::collect;
        struct Key { val value: i32 }
        impl PartialEq for Key {
            fn eq(self, other: Key) -> bool {
                collect();
                if self.value == 99 { other.value / (self.value - self.value) == 0 }
                else { self.value == other.value }
            }
        }
        impl Eq for Key {}
        impl Hash for Key { fn hash(self) -> i64 { collect() } }
        fn failing() -> i32 {
            val map = HashMap::new();
            map.insert(Key { value: 99 }, 1); map.insert(Key { value: 1 }, 2); 0
        }
        fn healthy() -> i32 {
            val map = HashMap::new();
            map.insert(Key { value: 1 }, 20); map.insert(Key { value: 2 }, 22);
            val set = HashSet::new();
            set.insert(Key { value: 1 }); set.insert(Key { value: 2 });
            set.insert(Key { value: 1 });
            val first_key = Key { value: 1 }; val second_key = Key { value: 2 };
            if set.len() != (2 as usize) || !set.contains(second_key) { return -1; }
            match map.get(first_key) {
                Some(first) => match map.get(second_key) { Some(second) => first + second, None => -2 },
                None => -3
            }
        }
    "#,
        Some(&module),
    );
    assert!(vm.execute(&loaded, "failing").is_err());
    vm.runtime().collect_garbage().unwrap();
    assert_eq!(
        vm.execute(&loaded, "healthy").unwrap().return_value,
        Value::I32(42)
    );
    assert!(count.get() > 10);
    let collected = vm.runtime().collect_garbage().unwrap();
    assert_eq!(collected.live_objects, 0);
}

#[test]
fn empty_hash_containers_reject_wrong_types_and_keep_the_selected_key_protocol() {
    let (mut vm, loaded) = compile(
        r#"
        struct Key { val value: i32 }
        impl PartialEq for Key { fn eq(self, other: Key) -> bool { self.value == other.value } }
        impl Eq for Key {}
        impl Hash for Key { fn hash(self) -> i64 { 7 } }
        fn empty_map() -> HashMap<i32, i32> { HashMap::new() }
        fn empty_set() -> HashSet<i32> { HashSet::new() }
        fn custom() -> HashMap<Key, i32> { HashMap::new() }
        fn key() -> Key { Key { value: 1 } }
    "#,
        None,
    );
    let map = vm.execute(&loaded, "empty_map").unwrap().return_value;
    let map_root = vm.runtime().root_value(map.clone()).unwrap();
    let set = vm.execute(&loaded, "empty_set").unwrap().return_value;
    let set_root = vm.runtime().root_value(set.clone()).unwrap();
    let custom = vm.execute(&loaded, "custom").unwrap().return_value;
    let custom_root = vm.runtime().root_value(custom.clone()).unwrap();
    let key = vm.execute(&loaded, "key").unwrap().return_value;
    let key_root = vm.runtime().root_value(key.clone()).unwrap();
    let Value::Map(map_id) = map else {
        panic!("map");
    };
    let Value::Set(set_id) = set else {
        panic!("set");
    };
    let Value::Map(custom_id) = custom else {
        panic!("custom map");
    };
    let heap = vm.runtime().gc();
    assert!(
        heap.map_insert(map_id, Value::Bool(true), Value::I32(1))
            .is_err()
    );
    assert!(
        heap.map_insert(map_id, Value::I32(1), Value::Bool(true))
            .is_err()
    );
    assert!(heap.set_insert(set_id, Value::Bool(true)).is_err());
    assert_eq!(heap.map_len(map_id), Some(0));
    assert_eq!(heap.set_len(set_id), Some(0));
    assert!(heap.ensure_key_mode(&custom, false).is_err());
    assert!(
        heap.map_insert(custom_id, key.clone(), Value::I32(42))
            .is_err()
    );
    heap.custom_insert(&custom, 7, -1, key, Value::I32(42))
        .unwrap();
    heap.map_clear(custom_id).unwrap();
    assert!(heap.ensure_key_mode(&custom, false).is_err());
    assert_eq!(heap.map_len(custom_id), Some(0));
    drop((map_root, set_root, custom_root, key_root));
}

#[test]
fn native_cursor_keeps_its_source_alive_and_shares_position_across_calls() {
    use std::cell::RefCell;
    let language = LanguageContracts::default();
    let mut builder = ModuleBuilder::new("example::cursors", &language);
    let hold = builder
        .define_function(FunctionDecl::new("hold").returns(language.collection_cursor(Type::i32())))
        .unwrap();
    let slot = Rc::new(RefCell::new(None::<kagari_runtime::gc::RootedValue>));
    let capture = slot.clone();
    builder
        .bind_with(
            hold,
            NativeBinding::new(vec![], Codec::Iterator, move |_| {
                capture
                    .borrow()
                    .as_ref()
                    .map(|root| root.value())
                    .ok_or_else(|| RuntimeError::module_validation("missing retained cursor"))
            }),
        )
        .unwrap();
    let module = builder.finish().unwrap();
    let (mut vm, loaded) = compile(
        r#"
        use example::cursors::hold;
        fn cursor() -> CollectionCursor<i32> { [20, 22].iter() }
        fn first() -> i32 { match hold().next() { Some(value) => value, None => -1 } }
        fn rest() -> i32 { var total = 0; for value in hold() { total = total + value; } total }
    "#,
        Some(&module),
    );
    let value = vm.execute(&loaded, "cursor").unwrap().return_value;
    *slot.borrow_mut() = Some(vm.runtime().root_value(value).unwrap());
    vm.runtime().collect_garbage().unwrap();
    assert_eq!(
        vm.execute(&loaded, "first").unwrap().return_value,
        Value::I32(20)
    );
    vm.runtime().collect_garbage().unwrap();
    assert_eq!(
        vm.execute(&loaded, "rest").unwrap().return_value,
        Value::I32(22)
    );
    assert_eq!(
        vm.execute(&loaded, "rest").unwrap().return_value,
        Value::I32(0)
    );
    *slot.borrow_mut() = None;
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
}

#[test]
fn native_constructor_supplies_a_traced_payload_without_a_default_factory() {
    let language = LanguageContracts::default();
    let mut builder = ModuleBuilder::new("example::provided", &language);
    let mut declaration = builder.define_type("Counter");
    declaration
        .native_storage(NativeStorage::payload::<TracedCounter>())
        .unwrap();
    let counter = declaration.finish().unwrap();
    let ty = counter.apply([]).unwrap();
    let dropped = Rc::new(Cell::new(0));
    let new = builder
        .define_function(
            FunctionDecl::new("new")
                .parameter("values", language.array_list(Type::i32()))
                .returns(ty.clone()),
        )
        .unwrap();
    let capture = dropped.clone();
    builder
        .bind_with(
            new,
            NativeBinding::new(vec![Codec::Sequence], counter.codec(), move |cx| {
                cx.allocate_result_payload(TracedCounter {
                    values: cx.argument(0)?,
                    dropped: capture.clone(),
                })
            }),
        )
        .unwrap();
    let wrong = builder
        .define_function(FunctionDecl::new("missing_payload").returns(ty.clone()))
        .unwrap();
    builder
        .bind_with(
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
        .bind_with(
            read,
            NativeBinding::new(
                vec![counter.codec()],
                Codec::Scalar(Type::i32().abi().clone()),
                |cx| {
                    cx.with_payload::<TracedCounter, _>(0, |payload| {
                        let Value::Array(id) = payload.values else {
                            return Err(RuntimeError::module_validation("counter child"));
                        };
                        cx.heap()
                            .array_get(id, 0)
                            .ok_or_else(|| RuntimeError::module_validation("counter element"))
                    })
                },
            ),
        )
        .unwrap();
    let module = builder.finish().unwrap();
    let (mut vm, loaded) = compile(
        r#"
        use example::provided::{Counter, new, read, missing_payload};
        fn main() -> Counter { new([42]) }
        fn healthy() -> i32 { read(new([42])) }
        fn invalid() -> Counter { missing_payload() }
    "#,
        Some(&module),
    );
    let error = vm.execute(&loaded, "invalid").unwrap_err();
    let VmError::RuntimeError(error) = error.cause() else {
        panic!("expected storage allocation error");
    };
    assert!(error.message().contains("explicitly supplied payload"));
    let value = vm.execute(&loaded, "main").unwrap().return_value;
    let root = vm.runtime().root_value(value).unwrap();
    vm.runtime().collect_garbage().unwrap();
    assert_eq!(dropped.get(), 0);
    assert_eq!(
        vm.execute(&loaded, "healthy").unwrap().return_value,
        Value::I32(42)
    );
    drop(root);
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
    assert_eq!(dropped.get(), 2);
}

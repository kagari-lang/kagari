use crate::BytecodeArtifact;
use crate::ExecutionContext;
use crate::KagariEngine;
use kagari_common::SourceFile;
use kagari_runtime::value::Value;

fn execute(source: &str) {
    let mut config = kagari_embed::EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let engine = KagariEngine::new(config);
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("lazy-iterators.kgr", source),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    for (encoded, jit) in [(false, false), (true, false), (true, true)] {
        let artifact = if encoded {
            BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap()
        } else {
            artifact.clone()
        };
        let mut context = ExecutionContext::default();
        context.capabilities.jit = jit;
        context.language_profile.allow_jit = jit;
        context.jit_policy = if jit {
            kagari_embed::JitPolicy::Enabled
        } else {
            kagari_embed::JitPolicy::Disabled
        };
        let mut runtime = engine.runtime(context.clone());
        let loaded = runtime.load_program(artifact, Default::default()).unwrap();
        let result = if jit {
            let mut backend = kagari_codegen_cranelift::CraneliftBackend::for_host().unwrap();
            runtime.execute_with_backend(&loaded, "main", &[], &context, &mut backend)
        } else {
            runtime.execute(&loaded, "main", &[], &context)
        }
        .unwrap();
        assert_eq!(result.return_value, Value::I32(42));
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
    }
}

#[test]
fn terminals_short_circuit_and_preserve_shared_progress() {
    execute(
        r#"
    fn main() -> i32 {
        val values = [1, 20, 22, 99];
        val iter = values.iter().map(|x| x);
        std::debug::assert_eq(iter.find(|x| x == 20), Some(20), "find");
        std::debug::assert(iter.any(|x| x == 22), "any");
        std::debug::assert(!iter.all(|x| x < 90), "all");
        std::debug::assert_eq(iter.count(), "".len_bytes(), "end");
        values.push(200);
        val empty = [1].iter().take("".len_bytes());
        std::debug::assert(empty.all(|x| false), "empty all");
        std::debug::assert(!empty.any(|x| true), "empty any");
        val count = [20, 22].iter().count();
        std::debug::assert_eq(count, "ab".len_bytes(), "count");
        var sum = 0;
        [20, 22].iter().for_each(|x| { sum += x; });
        std::debug::assert_eq(sum, 42, "for_each");
        [20, 22].iter().fold(0, |sum, x| sum + x)
    }
    "#,
    );
}

#[test]
fn partition_and_group_by_build_fresh_typed_destinations() {
    execute(
        r#"
    fn main() -> i32 {
        val split: (ArrayList<i32>, ArrayList<i32>) = [20, 1, 22, 3].iter().partition(|x| x % 2 == 0);
        std::debug::assert_eq(split[1][1], 3, "rejected order");
        val groups = [20, 1, 22, 3].iter().group_by(|x| x % 2);
        std::debug::assert_eq(groups.get(1).unwrap_or([0])[1], 3, "group order");
        std::debug::assert_eq(groups.keys()[0], 0, "first key order");
        val group = groups.get(0).unwrap_or([0]);
        group.push(99);
        split[0][0] + split[0][1]
    }
    "#,
    );
}

#[test]
fn grouping_uses_custom_key_protocol_and_partition_uses_user_destination() {
    execute(
        r#"
    struct Key { val value: i32 }
    impl PartialEq for Key { fn eq(self, other: Key) -> bool { self.value == other.value } }
    impl Eq for Key {}
    impl Hash for Key { fn hash(self) -> i64 { 0.hash() } }
    struct Total { val value: i32 }
    impl FromIterator<i32> for Total {
        fn from_iter<I: Iterable<Item = i32>>(source: I) -> Self {
            var value = 0; for item in source { value += item; } Total { value }
        }
    }
    fn main() -> i32 {
        val groups = [20, 1, 22, 3].iter().group_by(|x| Key { value: x % 2 });
        std::debug::assert_eq(groups.len(), "ab".len_bytes(), "collisions");
        std::debug::assert_eq(groups.get(Key { value: 0 }).unwrap_or([0])[1], 22, "custom equality");
        val split: (Total, Total) = [20, 1, 22, 3].iter().partition(|x| x % 2 == 0);
        std::debug::assert_eq(split[1].value, 4, "user destination");
        split[0].value
    }
    "#,
    );
}

#[test]
fn pipelines_reject_invalid_callbacks_keys_and_removed_helpers() {
    for source in [
        "fn main(){[1].iter().filter(|x| x);}",
        "fn main(){[1].iter().group_by(|x| 1.0);}",
        "fn main(){val x:(ArrayList<String>,ArrayList<String>)=[1].iter().partition(|x|true);}",
        "fn main(){[1].iter().filter_map(|x|{val unknown=None;if x==0 {None}else{Some(x)}});}",
        "fn main(){std::iter::len([1]);}",
        "fn main(){std::iter::for_each([1],|x|{});}",
    ] {
        assert!(
            KagariEngine::default()
                .compile_to_artifact(
                    SourceFile::new("invalid-pipeline.kgr", source),
                    Default::default(),
                    Default::default()
                )
                .is_err(),
            "{source}"
        );
    }
}

#[test]
fn native_from_iter_supports_qualified_declaration_paths() {
    execute(
        r#"
    fn main()->i32 {
        val values=std::array::ArrayList::from_iter([20,22]);
        val unique=std::set::LinkedHashSet::from_iter(values);
        val map=std::map::LinkedHashMap::from_iter([(1,20),(2,22)]);
        std::debug::assert_eq(unique.len(),values.len(),"qualified constructor");
        map.get(1).unwrap_or(0)+map.get(2).unwrap_or(0)
    }
    "#,
    );
}

#[test]
fn explicit_iterator_default_override_uses_the_script_implementation() {
    execute(
        r#"
    struct Count {}
    impl Iterator for Count {
        type Item=i32;
        fn next(self)->Option<i32>{None}
        fn count(self)->usize{"answer".len_bytes()}
    }
    fn size<I:Iterator>(source:I)->usize{source.count()}
    fn main()->i32{
        std::debug::assert_eq(size(Count{}),"answer".len_bytes(),"override");
        42
    }
    "#,
    );
}

#[test]
fn verifier_rejects_malformed_adapter_contracts_and_negative_usize_state() {
    use kagari_abi::operations::IterOp;
    use kagari_abi::scalar::BuiltinType;
    use kagari_abi::types::AbiType;
    use kagari_bytecode::BytecodeInstruction as I;
    use kagari_bytecode::ConstantOperand;
    let artifact = KagariEngine::default()
        .compile_to_artifact(
            SourceFile::new(
                "invalid-iter-wire.kgr",
                "fn main(){val a:ArrayList<(usize,i32)> = [1].iter().enumerate().collect();}",
            ),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    for corrupt in 0..3 {
        let mut program = artifact.program.clone();
        let function = &mut program.modules[0].functions[0];
        if corrupt == 2 {
            let register = *function
                .metadata
                .semantic
                .registers
                .iter()
                .find(|(_, ty)| **ty == AbiType::Builtin(BuiltinType::USize))
                .unwrap()
                .0;
            let value = function
                .instructions
                .iter_mut()
                .find_map(|i| match i {
                    I::LoadConst { dst, constant } if dst.index() == register => Some(constant),
                    _ => None,
                })
                .unwrap();
            *value = ConstantOperand::I64(-1);
        } else {
            let ty = function
                .instructions
                .iter_mut()
                .find_map(|i| match i {
                    I::Iter {
                        ty,
                        op: IterOp::FromClosure,
                        ..
                    } => Some(ty),
                    _ => None,
                })
                .unwrap();
            let AbiType::Tuple(fields) = ty else {
                panic!("adapter captures")
            };
            let AbiType::Function { params, result } = &mut fields[0] else {
                panic!("step signature")
            };
            if corrupt == 0 {
                params.push(AbiType::Builtin(BuiltinType::I32));
            } else {
                **result = AbiType::Builtin(BuiltinType::I32);
            }
        }
        assert!(
            kagari_bytecode::verify_program(&program).is_err(),
            "mutation {corrupt}"
        );
    }
}

#[test]
fn native_iteration_reads_live_slots_and_does_not_snapshot_items() {
    execute(
        r#"
    struct Item { val value: i32 }
    fn main() -> i32 {
        val source = [Item { value: 1 }, Item { value: 2 }];
        val iter = source.iter();
        source[0] = Item { value: 20 };
        val first = iter.next().unwrap_or(Item { value: 0 });
        source[1] = Item { value: 22 };
        val rest: ArrayList<Item> = iter.collect();
        val text: ArrayList<String> = "中😀é".iter().collect();
        std::debug::assert_eq(text[1], "😀", "scalar iteration");
        std::debug::assert_eq(text[2], "é", "UTF-8 progress");
        source.push(Item { value: 99 });
        first.value + rest[0].value
    }
    "#,
    );
}

#[test]
fn duplicate_iter_dependencies_and_deep_adapter_chains_release_guards() {
    execute(
        r#"
    fn main() -> i32 {
        val source = [20, 22];
        val iter = source.iter();
        val pairs: ArrayList<(i32,i32)> = iter.zip(iter).collect();
        std::debug::assert_eq(pairs[0][0] + pairs[0][1], 42, "shared zip");
        source.push(99);
        var deep = source.iter();
        var depth = 0;
        while depth < 1500 { deep = deep.map(|x| x); depth += 1; }
        std::debug::assert_eq(deep.take("".len_bytes()).count(), "".len_bytes(), "take zero");
        source.push(100);
        42
    }
    "#,
    );
}

#[test]
fn adapter_traps_budgets_and_changed_sources_leave_runtime_usable() {
    let engine = KagariEngine::default();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "pipeline-failure.kgr",
                r#"
        fn trap(){ [1].iter().map(|x| x / 0).for_each(|x| {}); }
        fn structural(){ val a=[1]; a.iter().filter(|x| {a.push(2);true}).count(); }
        fn changed(){ val a=[1,2];val i=a.iter().map(|x| x);i.any(|x|true);a.push(3);i.next(); }
        struct Forever {}
        impl Iterator for Forever { type Item=i32; fn next(self)->Option<i32>{Some(1)} }
        fn exhaust(){ Forever{}.filter(|x|false).count(); }
        fn inner_trap(){ [1].iter().flat_map(|x| [x].iter().map(|y| y / 0)).count(); }
        fn inner_structural(){ val a=[1]; [a].iter().flatten().inspect(|x| { a.push(2); }).count(); }
        fn sum_overflow()->i32 { [2147483647, 1].iter().sum() }
        fn product_overflow()->i32 { [2147483647, 2].iter().product() }
        fn narrow_sum_overflow()->i8 {
            val empty: ArrayList<i8> = ArrayList::new();
            val one: i8 = empty.iter().product();
            val values: ArrayList<i8> = ArrayList::new();
            var index = 0;
            while index < 128 { values.push(one); index += 1; }
            values.iter().sum()
        }
        fn narrow_product_overflow()->u8 {
            val empty: ArrayList<u8> = ArrayList::new();
            val one: u8 = empty.iter().product();
            val two: u8 = [one, one].iter().sum();
            [two, two, two, two, two, two, two, two].iter().product()
        }
        fn healthy()->i32 {42}
    "#,
            ),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime.load_program(artifact, Default::default()).unwrap();
    for entry in [
        "trap",
        "structural",
        "changed",
        "exhaust",
        "inner_trap",
        "inner_structural",
        "sum_overflow",
        "product_overflow",
        "narrow_sum_overflow",
        "narrow_product_overflow",
    ] {
        let mut options = context.clone();
        if entry == "exhaust" {
            options.resources.max_instruction_steps = Some(150);
        }
        let error = runtime.execute(&loaded, entry, &[], &options).unwrap_err();
        assert!(!format!("{error:?}").contains("UnsupportedExecution"));
        assert_eq!(runtime.runtime().gc().active_roots(), 0, "{entry}");
        assert!(runtime.runtime().execution_root().is_none());
        assert!(!runtime.runtime().is_quarantined());
        assert_eq!(
            runtime
                .execute(&loaded, "healthy", &[], &context)
                .unwrap()
                .return_value,
            Value::I32(42)
        );
    }
}

#[test]
fn rooted_pipeline_retains_captures_and_progress_across_execution_sessions() {
    use kagari_common::host_interface::{HostInterface, standard_log};
    use kagari_runtime::host::HostFunction;
    use std::{cell::RefCell, rc::Rc};
    let engine = KagariEngine::default();
    engine
        .set_host_interface(HostInterface {
            functions: vec![standard_log()],
            types: vec![],
            paths: vec![],
        })
        .unwrap();
    let mut context = ExecutionContext::default();
    context.language_profile.allow_host_calls = true;
    context.capabilities.host_calls = true;
    context.host_policy.allowed_host_functions = vec!["host.log".into()];
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "retained-pipeline.kgr",
                r#"
        struct Offset { val value: i32 }
        fn make()->Iter<i32>{val offset=Offset{value:1};[[19,21]].iter().flatten().map(|x|x+offset.value)}
        fn read(iter:Iter<i32>)->i32 {iter.next().unwrap_or(0)}
        fn main(){print("read");}
    "#,
            ),
            kagari_embed::CompileOptions {
                language_profile: context.language_profile,
            },
            Default::default(),
        )
        .unwrap();
    let read = artifact.program.modules[0]
        .functions
        .iter()
        .find(|f| f.name == "read")
        .unwrap()
        .id;
    let retained = Rc::new(RefCell::new(None::<kagari_runtime::gc::RootedValue>));
    let seen = Rc::new(RefCell::new(Vec::new()));
    let (input, output) = (retained.clone(), seen.clone());
    let mut runtime = engine.runtime(context.clone());
    runtime
        .register_host_function(HostFunction::new(standard_log(), move |context, _| {
            context.runtime().collect_garbage().unwrap();
            let root = context.runtime().execution_root().unwrap();
            let value = input.borrow().as_ref().unwrap().value();
            let value = kagari_vm::reenter(context, &root, read, &[value]).unwrap();
            context.runtime().collect_garbage().unwrap();
            output.borrow_mut().push(value.value());
            Ok(Value::Unit)
        }))
        .unwrap();
    let loaded = runtime.load_program(artifact, Default::default()).unwrap();
    let value = runtime
        .execute(&loaded, "make", &[], &context)
        .unwrap()
        .return_value;
    *retained.borrow_mut() = Some(runtime.runtime().root_value(value).unwrap());
    for _ in 0..3 {
        runtime.runtime().collect_garbage().unwrap();
        runtime.execute(&loaded, "main", &[], &context).unwrap();
    }
    assert_eq!(
        *seen.borrow(),
        vec![Value::I32(20), Value::I32(22), Value::I32(0)]
    );
    retained.borrow_mut().take();
    runtime.runtime().collect_garbage().unwrap();
    assert_eq!(runtime.runtime().gc().active_roots(), 0);
}

#[test]
fn native_iter_allocation_is_independent_of_source_length() {
    use kagari_abi::operations::IterOp;
    use kagari_abi::scalar::BuiltinType;
    use kagari_abi::types::AbiType;
    let engine = KagariEngine::default();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("iter-allocation.kgr", "fn main(){}"),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    let mut runtime = engine.runtime(Default::default());
    let loaded = runtime.load_program(artifact, Default::default()).unwrap();
    let rt = runtime.runtime();
    let array = rt.gc().alloc_array(vec![Value::I32(7); 10_000]).unwrap();
    let root = rt.root_value(Value::Array(array)).unwrap();
    let session = rt.begin_execution(&loaded, Default::default()).unwrap();
    let before = rt.gc().stats().allocation_units;
    let value = rt
        .iter_operation(
            &loaded,
            &root.value(),
            &AbiType::Array(
                Box::new(AbiType::Builtin(BuiltinType::I32)),
                kagari_common::collection::CollectionAccess::Mutable,
            ),
            IterOp::New,
        )
        .unwrap();
    let iter = rt.root_value(value).unwrap();
    assert!(rt.gc().stats().allocation_units - before <= 2);
    drop(session);
    drop(iter);
    drop(root);
    rt.collect_garbage().unwrap();
    assert_eq!(rt.gc().active_roots(), 0);
}

#[test]
fn adapters_are_lazy_shared_and_collect_without_intermediate_arrays() {
    execute(
        r#"
struct Calls {var count:i32}
fn main()->i32 {
    val calls=Calls{count:0}; val values=[1,20,2,22,100];
    val pipeline=values.iter().filter(|x| x>=20).map(|x| {calls.count+=1;x}).take([0,0].len());
    std::debug::assert_eq(calls.count,0,"construction is lazy");
    val alias=pipeline.iter();
    std::debug::assert_eq(alias.next(),Some(20),"first item");
    std::debug::assert_eq(pipeline.next(),Some(22),"shared progress");
    std::debug::assert_eq(alias.next(),None,"take stops");
    std::debug::assert_eq(calls.count,2,"no extra callback");
    values.push(200);
    val result:ArrayList<i32> = [0,20,0,22].iter().filter_map(|x| if x==0 {None}else{Some(x)}).collect();
    result[0]+result[1]
}
"#,
    );
}

#[test]
fn skip_enumerate_zip_and_chain_preserve_order() {
    execute(
        r#"
fn main()->i32 {
    val pipeline=[99,20,22].iter().skip([0].len()).enumerate().zip(["a","b"]);
    val pairs:ArrayList<((usize,i32),String)> = pipeline.collect();
    std::debug::assert_eq(pairs[0][0][0],"".len_bytes(),"zero index");
    std::debug::assert_eq(pairs[1][0][0],"a".len_bytes(),"next index");
    std::debug::assert_eq(pairs[1][1],"b","right input");
    val chained:ArrayList<i32> = [20].iter().chain([22]).collect();
    chained[0]+chained[1]
}
"#,
    );
}

#[test]
fn custom_iterators_use_native_defaults_and_generic_callbacks() {
    execute(
        r#"
struct Counter {var value:i32}
impl Iterator for Counter {type Item=i32;fn next(self)->Option<i32>{if self.value>22 {None}else{val n=self.value;self.value+=1;Some(n)}}}
fn transform<I:Iterator<Item=i32>>(source:I)->Iter<i32> {source.filter(|x|x!=21).map(|x|x)}
fn main()->i32 {val result:ArrayList<i32> = transform(Counter{value:20}).collect();result[0]+result[1]}
"#,
    );
}

#[test]
fn conditional_adapters_and_lookup_preserve_lazy_progress() {
    execute(
        r#"
fn main() -> i32 {
    var checks = 0;
    var observed = 0;
    val source = [1, 2, 3, 1, 9].iter();
    val items = source.skip_while(|x| { checks += 1; x < 3 }).inspect(|x| { observed += x; }).take_while(|x| x < 9);
    std::debug::assert_eq(checks, 0, "lazy predicate");
    val collected: ArrayList<i32> = items.collect();
    std::debug::assert_eq(collected.len(), "ab".len_bytes(), "suffix prefix");
    std::debug::assert_eq(checks, 3, "stops testing after rejection");
    std::debug::assert_eq(observed, 13, "includes rejected take item");
    std::debug::assert_eq(items.next(), None, "take remains ended");
    val other = [1, 20, 22, 9].iter();
    std::debug::assert_eq(other.find_map(|x| if x > 1 { Some(x) } else { None }), Some(20), "find map");
    std::debug::assert_eq(other.position(|x| x == 22), Some("".len_bytes()), "relative position");
    std::debug::assert_eq(other.nth("".len_bytes()), Some(9), "nth zero");
    std::debug::assert_eq(other.last(), None, "empty last");
    std::debug::assert_eq([1, 2, 3].iter().nth("abc".len_bytes()), None, "out of range");
    [20, 22].iter().reduce(|a, b| a + b).unwrap_or(0)
}
"#,
    );
}

#[test]
fn fused_custom_iterators_and_extrema_have_defined_ties() {
    execute(
        r#"
struct Pulse { var step: i32 }
impl Iterator for Pulse {
    type Item = i32;
    fn next(self) -> Option<i32> {
        self.step += 1;
        if self.step == 2 { None } else { Some(self.step) }
    }
}
struct Entry { val key: i32, val label: i32 }
fn main() -> i32 {
    val pulse = Pulse { step: 0 };
    val fused = pulse.fuse();
    std::debug::assert_eq(fused.next(), Some(1), "first");
    std::debug::assert_eq(fused.next(), None, "end");
    std::debug::assert_eq(fused.next(), None, "fused");
    std::debug::assert_eq(pulse.step, 2, "no resumption");
    val entries = [Entry { key: 1, label: 20 }, Entry { key: 1, label: 22 }];
    val first = entries.iter().min_by(|a, b| a.key.cmp(b.key));
    val last = entries.iter().max_by(|a, b| a.key.cmp(b.key));
    match (first, last) { (Some(a), Some(b)) => a.label + b.label, _ => 0 }
}
"#,
    );
}

#[test]
fn ordered_extrema_check_bounds_and_evaluate_keys_once() {
    execute(
        r#"
fn smallest<I: Iterator<Item = i32>>(items: I) -> Option<i32> { items.min() }
fn main() -> i32 {
    var calls = 0;
    val entries = [(2, 22), (1, 20), (1, 99)];
    val item = entries.iter().min_by_key(|x| { calls += 1; x[0] });
    std::debug::assert_eq(calls, 3, "once per key");
    std::debug::assert_eq(item, Some((1, 20)), "first minimum");
    std::debug::assert_eq(smallest([20, 22].iter()), Some(20), "generic bound");
    std::debug::assert_eq([20, 22].iter().max(), Some(22), "max");
    std::debug::assert_eq(entries.iter().max_by_key(|x| x[0]), Some((2, 22)), "max key");
    val empty: ArrayList<i32> = ArrayList::new();
    std::debug::assert_eq(empty.iter().min(), None, "empty");
    42
}
"#,
    );
    let engine = KagariEngine::default();
    for source in [
        "fn main() { [1.0, 2.0].iter().min(); }",
        "struct Entry { val x: i32 } fn main() { [Entry { x: 1 }].iter().min(); }",
        "fn main() { [1, 2].iter().max_by_key(|x| 1.0); }",
    ] {
        assert!(
            engine
                .compile_to_artifact(
                    SourceFile::new("invalid-extrema.kgr", source),
                    Default::default(),
                    Default::default()
                )
                .is_err()
        );
    }
}

#[test]
fn nested_iterators_are_lazy_resume_inner_progress_and_release_guards() {
    execute(
        r#"
fn main() -> i32 {
    var calls = 0;
    val first = [20, 22];
    val second = [99];
    val source = [first, second];
    val items = source.iter().flat_map(|x| { calls += 1; x });
    std::debug::assert_eq(calls, 0, "lazy outer");
    std::debug::assert_eq(items.next(), Some(20), "first inner");
    std::debug::assert_eq(calls, 1, "one inner created");
    std::debug::assert_eq(items.find(|x| true), Some(22), "resume current inner");
    first.push(7);
    second.push(100);
    source.push([0]);
    val empty: ArrayList<i32> = ArrayList::new();
    val nested = [empty, [20], empty, [22]].iter().flatten();
    std::debug::assert_eq(nested.reduce(|a, b| a + b), Some(42), "skip empty inners");
    val before = [1];
    val after = [2];
    var switched = 0;
    val released: ArrayList<i32> = [before, after].iter().flat_map(|x| {
        switched += 1;
        if switched == 2 { before.push(3); }
        x
    }).collect();
    std::debug::assert_eq(released.len(), "ab".len_bytes(), "close old inner before callback");
    std::debug::assert_eq(before.len(), "ab".len_bytes(), "old inner mutable again");
    42
}
"#,
    );
}

#[test]
fn flatten_accepts_custom_iterables_and_rejects_scalar_items() {
    execute(
        r#"
struct Pair { val a: i32, val b: i32 }
impl Iterable for Pair {
    type Item = i32;
    type Iter = Iter<i32>;
    fn iter(self) -> Iter<i32> { [self.a, self.b].iter() }
}
fn expand<I: Iterator<Item = Pair>>(source: I) -> ArrayList<i32> { source.flatten().collect() }
fn main() -> i32 {
    val values = expand([Pair { a: 20, b: 22 }].iter());
    values[0] + values[1]
}
"#,
    );
    let engine = KagariEngine::default();
    for source in [
        "fn main() { [1, 2].iter().flatten(); }",
        "fn main() { [1, 2].iter().flat_map(|x| x); }",
    ] {
        assert!(
            engine
                .compile_to_artifact(
                    SourceFile::new("bad-flatten.kgr", source),
                    Default::default(),
                    Default::default()
                )
                .is_err()
        );
    }
}

#[test]
fn aggregation_protocols_support_numeric_identities_and_user_targets() {
    execute(
        r#"
struct Total { val value: i32 }
impl Sum<i32> for Total {
    fn sum<I: Iterable<Item = i32>>(source: I) -> Self {
        var value = 0;
        for item in source { value += item; }
        Total { value }
    }
}
fn aggregate<I: Iterator<Item = i32>>(source: I) -> Total { source.sum() }
fn iterable_sum<I: Iterable<Item = i32>>(source: I) -> i32 { source.iter().map(|x| x).sum() }
fn iterable_fold<I: Iterable<Item = i32>>(source: I) -> i32 { source.iter().fold(0, |a, b| a + b) }
fn main() -> i32 {
    val empty: ArrayList<i32> = ArrayList::new();
    val zero: i32 = empty.iter().sum();
    val one: i32 = empty.iter().product();
    std::debug::assert_eq(zero, 0, "empty sum");
    std::debug::assert_eq(one, 1, "empty product");
    val product: i32 = [6, 7].iter().product();
    std::debug::assert_eq(product, 42, "product");
    val floats: f32 = [1.0, 2.0].iter().sum();
    std::debug::assert_eq(floats, 3.0, "float sum");
    val doubles: ArrayList<f64> = ArrayList::new();
    val double_one: f64 = doubles.iter().product();
    std::debug::assert_eq(f"{double_one}", "1", "double identity");
    val sizes: usize = ["a".len_bytes(), "ab".len_bytes()].iter().sum();
    std::debug::assert_eq(sizes, "abc".len_bytes(), "usize sum");
    val unsigned: ArrayList<u8> = ArrayList::new();
    val unsigned_one: u8 = unsigned.iter().product();
    std::debug::assert_eq(f"{unsigned_one}", "1", "unsigned identity");
    std::debug::assert_eq(iterable_sum([20, 22]), 42, "associated bounds on iterable");
    std::debug::assert_eq(iterable_fold([20, 22]), 42, "associated callback types");
    val total = aggregate([20, 22].iter());
    std::debug::assert_eq(Total::sum([20, 22]).value, 42, "qualified aggregate");
    total.value
}
"#,
    );
}

#[test]
fn fallible_collection_short_circuits_and_defers_custom_construction() {
    execute(
        r#"
struct Total { val value: i32 }
impl FromIterator<i32> for Total {
    fn from_iter<I: Iterable<Item = i32>>(source: I) -> Self {
        var value = 0;
        for item in source { value += item; }
        Total { value }
    }
}
struct Explodes { val value: i32 }
impl FromIterator<i32> for Explodes {
    fn from_iter<I: Iterable<Item = i32>>(source: I) -> Self {
        Explodes { value: 1 / 0 }
    }
}
fn collect_checked<I: Iterator<Item = Result<i32, String>>, C: FromIterator<i32>>(source: I) -> Result<C, String> { source.collect() }
fn read(x: i32) -> Result<i32, String> { if x < 0 { Err("negative") } else { Ok(x) } }
fn main() -> i32 {
    var calls = 0;
    val values = [20, -1, 22];
    val iter = values.iter().map(|x| { calls += 1; read(x) });
    val failed: Result<Explodes, String> = iter.collect();
    std::debug::assert(failed.is_err(), "failure without destination constructor");
    std::debug::assert_eq(calls, 2, "stopped at error");
    std::debug::assert_eq(iter.next(), Some(Ok(22)), "remaining source");
    std::debug::assert_eq(iter.next(), None, "exhausted after resumption");
    values.push(7);
    val none: Option<ArrayList<i32>> = [Some(20), None, Some(22)].iter().collect();
    std::debug::assert_eq(none, None, "option short circuit");
    val success: Result<Total, String> = collect_checked([20, 22].iter().map(|x| read(x)));
    val empty: ArrayList<Result<i32, String>> = ArrayList::new();
    val empty_result: Result<Total, String> = empty.iter().collect();
    std::debug::assert_eq(empty_result.map(|x| x.value), Ok(0), "empty constructs destination");
    val optional: Option<Total> = [Some(20), Some(22)].iter().collect();
    std::debug::assert_eq(optional.map(|x| x.value), Some(42), "custom option target");
    val nested_input: ArrayList<Result<Option<i32>, String>> = [Ok(Some(20)), Ok(Some(22))];
    val nested: Result<Option<ArrayList<i32>>, String> = nested_input.iter().collect();
    std::debug::assert(nested.is_ok(), "nested lifting");
    success.map(|x| x.value).unwrap_or(0)
}
"#,
    );
}

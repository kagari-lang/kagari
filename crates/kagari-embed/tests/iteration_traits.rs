use kagari_common::{SourceFile, collection::CollectionAccess};
use kagari_embed::{BytecodeArtifact, ExecutionContext, KagariEngine, program::PreparedProgram};
use kagari_runtime::value::Value;

fn execute(source: &str) {
    let mut config = kagari_embed::EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let engine = KagariEngine::new(config);
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("standard-traits.kgr", source),
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
        let loaded_program =
            PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap();
        let loaded = runtime
            .load_program(&loaded_program, Default::default())
            .unwrap();
        let result = if jit {
            let mut backend = kagari_codegen_cranelift::CraneliftBackend::for_host().unwrap();
            let prepared = runtime
                .prepare_native(
                    &loaded_program,
                    &loaded,
                    "main",
                    &mut backend,
                    &context.cancellation,
                )
                .unwrap();
            runtime.execute_prepared(&loaded, "main", &[], &context, &prepared)
        } else {
            runtime.execute(&loaded, "main", &[], &context)
        }
        .unwrap();
        assert_eq!(result.return_value, Value::I32(42));
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
    }
}

#[test]
fn custom_iterators_and_iterables_use_static_calls() {
    execute(
        r#"
struct Counter {var value:i32,val end:i32}
impl Iterator for Counter {type Item=i32;fn next(self)->Option<i32>{if self.value>=self.end {None}else{val value=self.value;self.value+=1;Some(value)}}}
struct Range {val start:i32,val end:i32}
impl Iterable for Range {type Item=i32;type Iter=Counter;fn iter(self)->Counter{Counter{value:self.start,end:self.end}}}
fn sum<C:Iterable<Item=i32>>(values:C)->i32 {var total=0;for value in values {if value==0 {continue;}total+=value;if total==42 {break;}}total}
fn main()->i32 {val a=Range{start:0,end:7};val b=Counter{value:0,end:7};sum(a)+sum(b)}
"#,
    );
}

#[test]
fn numeric_aggregation_uses_checked_native_and_user_iterable_conversion() {
    execute(
        r#"
struct Counter {val items:ArrayList<i32>,var index:usize}
impl Iterator for Counter {type Item=i32;fn next(self)->Option<i32>{if self.index>=self.items.len(){None}else{val item=self.items[self.index];self.index+=1;Some(item)}}}
struct Wrap {val items:ArrayList<i32>}
impl Iterable for Wrap {type Item=i32;type Iter=Counter;fn iter(self)->Counter{Counter{items:self.items,index:0}}}
fn aggregate<I:Iterable<Item=i32>,T:Sum<i32>>(source:I)->T {T::sum(source)}
fn main()->i32 {
    std::debug::assert_eq(i8::sum([20i8,22i8]),42i8,"narrow aggregate");
    val empty:ArrayList<i64> = [];
    std::debug::assert_eq(i64::product(empty),1i64,"empty identity");
    val total:i32 = aggregate(Wrap{items:[20,22]});
    val dynamic:Iterable<Item=i32,Iter=Counter> = Wrap{items:[2,3,7]};
    std::debug::assert_eq(i32::product(dynamic),42,"dynamic conversion");
    val list:List<i32> = [20,22];
    std::debug::assert_eq(i32::sum(list),42,"dynamic list");
    val storage=[20,22];val view:[i32] = storage;
    std::debug::assert_eq(i32::sum(view),42,"readonly source");storage.push(99);
    total
}
"#,
    );
}

#[test]
fn collect_uses_from_iterator_for_native_and_user_defined_destinations() {
    execute(
        r#"
struct Total { val value: i32 }
impl FromIterator<i32> for Total {
    fn from_iter<I: Iterable<Item=i32>>(source:I)->Self {
        var value=0; for item in source {value+=item;} Total{value}
    }
}
fn build<I:Iterator<Item=i32>, C:FromIterator<i32>>(source:I)->C {source.collect()}
fn main()->i32 {
    val array:ArrayList<i32> = [20,22].iter().collect();
    val mutable:ArrayList<i32> = array.iter().collect(); mutable.push(20);
    val set:LinkedHashSet<i32> = mutable.iter().collect();
    val writable:LinkedHashSet<i32> = mutable.iter().collect(); writable.insert(22);
    val map:LinkedHashMap<String,i32> = [("key",20),("key",22)].iter().collect();
    val writable_map:LinkedHashMap<String,i32> = map.iter().collect();
    writable_map.insert("other",20);
    std::debug::assert_eq(set.len(),[0,0].len(),"deduplication");
    std::debug::assert_eq(map.get("key"),Some(22),"last value wins");
    val total:Total=build(array.iter());
    std::debug::assert_eq(Total::from_iter(array).value,42,"associated construction");
    val copied=ArrayList::from_iter(array.iter());
    val copied_set=LinkedHashSet::from_iter(copied);
    val copied_map=LinkedHashMap::from_iter([("answer",42)]);
    std::debug::assert_eq(copied_set.len(),[0,0].len(),"inferred set constructor");
    std::debug::assert_eq(copied_map.get("answer"),Some(42),"inferred map constructor");
    total.value
}


"#,
    );
}

#[test]
fn native_iterators_support_generic_bounds_and_unicode() {
    execute(
        r#"
fn sum<C:Iterable<Item=i32>>(values:C)->i32 {var total=0;for value in values {total+=value;}total}
fn first<I:Iterator<Item=i32>>(values:I)->i32 {match values.next(){Some(x)=>x,None=>0}}
fn main()->i32 {
    val a=[20,22]; val iter:Iter<i32> =a.iter();
    std::debug::assert_eq(first(iter),20,"next");
    std::debug::assert_eq(first(iter),22,"shared iter");
    std::debug::assert_eq(first(iter),0,"exhausted");
    a.push(5);
    val scores:LinkedHashMap<String,i32> =LinkedHashMap::new();scores.insert("a",20);scores.insert("b",22);
    var total=0;for (key,value) in scores {total+=value;}
    std::debug::assert_eq(total,42,"map");
    val values:LinkedHashSet<i32> =LinkedHashSet::new();values.insert(20);values.insert(22);
    std::debug::assert_eq(sum(values),42,"set");
    var count=0;for ch in "中😀".iter(){count+=1;}
    std::debug::assert_eq(count,2,"unicode");
    sum([20,22])
}
"#,
    );
}

#[test]
fn breaking_native_loops_releases_guards_and_allows_resuming() {
    execute(
        r#"
fn main()->i32 {
    val a=[20,22]; val iter=a.iter();var total=0;
    for value in iter {total+=value;break;}
    for value in iter {total+=value;}
    a.push(1);
    for value in a {break;}
    a.push(2);
    total
}
"#,
    );
}

#[test]
fn returning_from_native_loop_releases_its_guard_before_caller_resumes() {
    execute(
        r#"
fn head(values:ArrayList<i32>)->i32 {for x in values {return x;}0}
fn main()->i32 {val a=[20];val b=head(a);a.push(22);b+a[1]}
"#,
    );
}

#[test]
fn native_guards_release_on_failure_and_iter_handles_survive_gc() {
    use kagari_abi::{operations::IterOp, scalar::BuiltinType, types::AbiType};
    let mut config = kagari_embed::EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let engine = KagariEngine::new(config);
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "iterator-resources.kgr",
                r#"
fn fail()->i32{val a=[20];for x in a {a.push(1);}0}
fn nested()->i32{val a=[20];for x in a {for y in a {break;}a.push(1);}0}
fn manual()->i32{val a=[20];val c=a.iter();c.next();a.push(1);0}
fn trap()->i32{val a=[20];for x in a {return x/0;}0}
fn exhaust()->i32{val a=[20];for x in a {while true {}}0}
"#,
            ),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    let artifact = BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded_program =
        PreparedProgram::from_artifact(artifact.clone(), &Default::default(), &Default::default())
            .unwrap();
    let loaded = runtime
        .load_program(&loaded_program, Default::default())
        .unwrap();
    for entry in ["fail", "nested", "manual", "trap", "exhaust"] {
        let mut options = context.clone();
        if entry == "exhaust" {
            options.resources.max_instruction_steps = Some(40);
        }
        let error = runtime.execute(&loaded, entry, &[], &options).unwrap_err();
        assert!(!format!("{error:?}").contains("UnsupportedExecution"));
        assert!(!runtime.runtime().is_quarantined());
        assert_eq!(runtime.runtime().gc().active_roots(), 0, "{entry}");
    }
    let rt = runtime.runtime();
    let array = rt
        .gc()
        .alloc_array(vec![Value::I32(20), Value::I32(22)])
        .unwrap();
    let root = rt.root_value(Value::Array(array)).unwrap();
    let item = AbiType::Builtin(BuiltinType::I32);
    let ty = AbiType::Iter(Box::new(item.clone()));
    let cancellation = kagari_common::cancellation::CancellationToken::default();
    let options = kagari_runtime::ExecutionOptions {
        cancellation: cancellation.clone(),
        ..Default::default()
    };
    let session = rt.begin_execution(&loaded, options).unwrap();
    let value = rt
        .iter_operation(
            &loaded,
            &root.value(),
            &AbiType::Array(Box::new(item), CollectionAccess::Mutable),
            IterOp::New,
        )
        .unwrap();
    let iter = rt.root_value(value.clone()).unwrap();
    assert!(rt.gc().array_push(array, Value::I32(1)).is_err());
    cancellation.cancel();
    assert!(rt.gc_safepoint().is_err());
    drop(session);
    rt.collect_garbage().unwrap();
    for expected in [20, 22] {
        let session = rt.begin_execution(&loaded, Default::default()).unwrap();
        let Value::Enum(id) = rt
            .iter_operation(&loaded, &iter.value(), &ty, IterOp::Next)
            .unwrap()
        else {
            panic!("Option")
        };
        assert_eq!(
            rt.gc().enum_snapshot(id).unwrap().fields,
            vec![Value::I32(expected)]
        );
        drop(session);
    }
    // Between root calls guards are released; resumed iterators reject a changed structure.
    rt.gc().array_push(array, Value::I32(1)).unwrap();
    let session = rt.begin_execution(&loaded, Default::default()).unwrap();
    assert!(
        rt.iter_operation(&loaded, &iter.value(), &ty, IterOp::Next)
            .is_err()
    );
    drop(session);
    let mut other = engine.runtime(context);
    let other_loaded_program =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let other_loaded = other
        .load_program(&other_loaded_program, Default::default())
        .unwrap();
    let session = other
        .runtime()
        .begin_execution(&other_loaded, Default::default())
        .unwrap();
    assert!(
        other
            .runtime()
            .iter_operation(&other_loaded, &iter.value(), &ty, IterOp::Next)
            .is_err()
    );
    drop(session);
    drop(iter);
    rt.collect_garbage().unwrap();
    let session = rt.begin_execution(&loaded, Default::default()).unwrap();
    assert!(
        rt.iter_operation(&loaded, &value, &ty, IterOp::Next)
            .is_err()
    );
    drop(session);
}

#[test]
fn invalid_iterator_outputs_and_overrides_are_diagnostics() {
    for source in [
        "fn removed(value: Cursor<i32>) {}",
        "struct C{} impl Iterator for C {type Item=i32;fn next(self)->i32 {42}}",
        "struct C{} impl Iterable for C {type Item=i32;type Iter=i32;fn iter(self)->i32 {42}}",
        "struct C{} impl Iterator for C {type Item=i32;fn next(self)->Option<i32>{None}} impl Iterable for C {type Item=i32;type Iter=C;fn iter(self)->C {self}}",
        "struct C{} impl Iterator for C {type Item=String;fn next(self)->Option<String>{None}} struct R{} impl Iterable for R {type Item=i32;type Iter=C;fn iter(self)->C{C{}}}",
        "fn take<T:Iterable<Item=String>>(x:T){} fn main(){take([42]);}",
    ] {
        assert!(
            KagariEngine::default()
                .compile_to_artifact(
                    SourceFile::new("invalid-iterator.kgr", source),
                    Default::default(),
                    Default::default()
                )
                .is_err(),
            "{source}"
        );
    }
}

#[test]
fn native_iter_type_and_iterable_associated_type_resolve_independently() {
    execute(
        r#"
        struct Values { val items: List<i32> }
        impl Iterable for Values {
            type Item = i32;
            type Iter = Iter<i32>;
            fn iter(self) -> Self::Iter { self.items.iter() }
        }
        fn sum<I: Iterable<Item = i32>>(source: I) -> i32 {
            var total = 0;
            for item in source { total += item; }
            total
        }
        fn main() -> i32 {
            val values = Values { items: [20, 22] };
            val items: Iter<i32> = values.iter().map(|item| item);
            sum(items)
        }
    "#,
    );
}

#[test]
fn generic_iterator_bound_also_supplies_for_and_nested_associated_outputs() {
    execute(
        r#"
struct Counter{var value:i32}
impl Iterator for Counter{type Item=i32;fn next(self)->Option<i32>{if self.value>0 {val n=self.value;self.value=0;Some(n)}else{None}}}
fn consume<T:Iterator<Item=i32>>(it:T)->i32{var total=0;for value in it {total+=value;}total}
struct Wrap<T>{val inner:T}
impl<T:Iterator<Item=i32>> Iterable for Wrap<T>{type Item=i32;type Iter=T;fn iter(self)->T{self.inner}}
fn main()->i32 {val a=Wrap{inner:Counter{value:21}};var total=consume(Counter{value:21});for value in a {total+=value;}total}
"#,
    );
}

#[test]
fn derived_iterable_satisfies_nested_impl_bounds() {
    execute(
        r#"
struct Counter{var value:i32}
impl Iterator for Counter{type Item=i32;fn next(self)->Option<i32>{if self.value>0 {val n=self.value;self.value=0;Some(n)}else{None}}}
trait Sum{fn sum(self)->i32;}
struct Wrap<T>{val inner:T}
impl<T:Iterable<Item=i32>> Sum for Wrap<T>{fn sum(self)->i32{var total=0;for x in self.inner {total+=x;}total}}
fn main()->i32{Wrap{inner:Counter{value:42}}.sum()}
"#,
    );
}

#[test]
fn iter_creates_fresh_collection_progress_but_preserves_iterator_aliases() {
    execute(
        r#"
fn main()->i32 {
    val values=[20,22]; val first=values.iter(); val second=values.iter();
    val alias=first.iter();
    std::debug::assert_eq(first.next(),Some(20),"first step");
    std::debug::assert_eq(alias.next(),Some(22),"shared progress");
    std::debug::assert_eq(second.next(),Some(20),"independent progress");
    std::debug::assert_eq(alias.next(),None,"exhausted");
    std::debug::assert_eq(second.next(),Some(22),"second step");
    std::debug::assert_eq(second.next(),None,"second exhausted");
    values.push(42); values[2]
}
"#,
    );
    for source in [
        "fn main(){[1].into_iter();}",
        "fn consume<I:IntoIterator>(values:I){}",
    ] {
        assert!(
            KagariEngine::default()
                .compile_to_artifact(
                    SourceFile::new("removed-iterator-api.kgr", source),
                    Default::default(),
                    Default::default(),
                )
                .is_err(),
            "{source}"
        );
    }
}

#[test]
fn malformed_native_iter_operations_are_rejected_before_execution() {
    use kagari_abi::{
        callable::{EngineNativeBinding, NativeCall},
        scalar::BuiltinType,
        standard::{bindings::NativeProtocolMethod, surface::StandardEnum},
        types::AbiType,
    };
    use kagari_bytecode::{BytecodeInstruction, CallTarget};
    let artifact = KagariEngine::default()
        .compile_to_artifact(
            SourceFile::new(
                "iter-wire.kgr",
                "fn main()->i32 {var total=0;for x in [20,22]{total+=x;}total}",
            ),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    let root = artifact.program.root.index();
    let import = artifact.program.modules[root]
        .engine_imports
        .iter()
        .position(|import| {
            import.binding == EngineNativeBinding::Protocol(NativeProtocolMethod::CollectionIter)
        })
        .unwrap();
    for corrupt in 0..3 {
        let mut program = artifact.program.clone();
        match corrupt {
            0 => {
                program.modules[root].engine_imports[import]
                    .signature
                    .params[0] = AbiType::Builtin(BuiltinType::I32)
            }
            1 => {
                let instruction = program.modules[root]
                    .functions
                    .iter_mut()
                    .flat_map(|function| &mut function.instructions)
                    .find(|instruction| {
                        matches!(instruction, BytecodeInstruction::Call {
                        callee: CallTarget::Native(NativeCall::Engine(id)), ..
                    } if id.index() == import)
                    })
                    .unwrap();
                let BytecodeInstruction::Call { args, .. } = instruction else {
                    unreachable!()
                };
                args.clear();
            }
            _ => {
                program.modules[root].engine_imports[import]
                    .signature
                    .params[0] = AbiType::Array(
                    Box::new(AbiType::StandardEnum {
                        kind: StandardEnum::Option,
                        args: vec![],
                    }),
                    CollectionAccess::Mutable,
                )
            }
        }
        assert!(kagari_bytecode::verify_program(&program).is_err());
        assert!(BytecodeArtifact::from_program(program, Default::default()).is_err());
    }
}

#[test]
fn for_evaluates_source_and_conversion_once_and_stops_next_at_break() {
    execute(
        r#"
struct Calls{var source:i32,var into:i32,var next:i32}
struct Range{val calls:Calls}
struct Counter{val calls:Calls}
fn make(calls:Calls)->Range{calls.source+=1;Range{calls}}
impl Iterable for Range{type Item=i32;type Iter=Counter;fn iter(self)->Counter{self.calls.into+=1;Counter{calls:self.calls}}}
impl Iterator for Counter{type Item=i32;fn next(self)->Option<i32>{self.calls.next+=1;Some(21)}}
fn main()->i32{val calls=Calls{source:0,into:0,next:0};var total=0;for x in make(calls){total+=x;if total==42 {break;}}std::debug::assert_eq(calls.source,1,"source once");std::debug::assert_eq(calls.into,1,"into once");std::debug::assert_eq(calls.next,2,"no next after break");total}
"#,
    );
}

#[test]
fn collection_protocol_bounds_reject_invalid_sources_and_destinations() {
    for source in [
        "fn main(){ val a: List<String> = [1].iter().collect(); }",
        "fn main(){ val a: Set<f32> = [1.0].iter().collect(); }",
        "fn main(){ val a: Map<i32,i32> = [1].iter().collect(); }",
        "fn main(){ val a: i32 = [1].iter().collect(); }",
        "fn main(){ [1].iter().collect(); }",
        "fn main(){ List<i32>::from_iter(1); }",
        "impl FromIterator<i32> for ArrayList<i32>{ fn from_iter<I:Iterable<Item=i32>>(source:I)->Self { [] } }",
        "struct C{} impl FromIterator<i32> for C{ fn from_iter<I:Iterable<Item=String>>(source:I)->Self { C{} } }",
    ] {
        assert!(
            KagariEngine::default()
                .compile_to_artifact(
                    SourceFile::new("invalid-collection-protocol.kgr", source),
                    Default::default(),
                    Default::default(),
                )
                .is_err(),
            "{source}"
        );
    }
}

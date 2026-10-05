mod support;
use kagari_bytecode::program::verify_program;
use kagari_embed::{
    BytecodeArtifact,
    context::{ExecutionContext, JitPolicy},
    engine::{EngineConfig, KagariEngine},
    program::PreparedProgram,
};
use kagari_runtime::{session::ExecutionOptions, value::Value};
use kagari_source::source::SourceFile;
use kagari_types::collection::CollectionAccess;

fn execute(source: &str) {
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let engine = KagariEngine::new(config);
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("standard-traits.kgr", source),
            Default::default(),
        )
        .unwrap();
    for (encoded, jit) in [(false, false), (true, false), (true, true)] {
        let artifact = if encoded {
            BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap()
        } else {
            artifact.clone()
        };
        let context = ExecutionContext {
            jit_policy: if jit {
                JitPolicy::Enabled
            } else {
                JitPolicy::Disabled
            },
            ..Default::default()
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
        assert_eq!(
            result
                .return_value
                .value(runtime.runtime().gc())
                .expect("retained execution result"),
            Value::I32(42)
        );
        drop(result);
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
fn native_iterators_support_generic_bounds_and_default_collections() {
    execute(
        r#"use std::collections::{HashMap, HashSet};
use std::iter::{CollectionCursor};

fn sum<C:Iterable<Item=i32>>(values:C)->i32 {var total=0;for value in values {total+=value;}total}
fn first<I:Iterator<Item=i32>>(values:I)->i32 {match values.next(){Some(x)=>x,None=>0}}
fn main()->i32 {
    val a=[20,22]; val iter:CollectionCursor<i32> =a.iter();
    if first(iter) != 20 { return 0; }
    if first(iter) != 22 { return 0; }
    if first(iter) != 0 { return 0; }
    a.push(5);
    val scores:HashMap<String,i32> =HashMap::new();scores.insert("a",20);scores.insert("b",22);
    var total=0;for (key,value) in scores {total+=value;}
    if total != 42 { return 0; }
    val values:HashSet<i32> =HashSet::new();values.insert(20);values.insert(22);
    if sum(values) != 42 { return 0; }
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
fn head(values:Vec<i32>)->i32 {for x in values {return x;}0}
fn main()->i32 {val a=[20];val b=head(a);a.push(22);b+a[1]}
"#,
    );
}

#[test]
fn native_guards_release_on_failure_and_iter_handles_survive_gc() {
    use {
        kagari_contract::operations::IterOp,
        kagari_types::{scalar::BuiltinType, ty::Ty},
    };
    let mut config = EngineConfig::default();
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
        let options = context.clone();
        let cancellation =
            (entry == "exhaust").then(|| support::cancel_after(runtime.runtime(), &loaded, 30));
        let error = runtime.execute(&loaded, entry, &[], &options).unwrap_err();
        drop(cancellation);
        assert!(!format!("{error:?}").contains("UnsupportedExecution"));
        assert!(!runtime.runtime().is_quarantined());
        assert_eq!(runtime.runtime().gc().active_roots(), 0, "{entry}");
    }
    let rt = runtime.runtime();
    let array = rt
        .alloc_array(
            &loaded,
            Ty::Builtin(BuiltinType::I32),
            vec![Value::I32(20), Value::I32(22)],
        )
        .unwrap();
    let root = rt.root_value(Value::Array(array)).unwrap();
    let item = Ty::Builtin(BuiltinType::I32);
    let ty = Ty::Iter(Box::new(item.clone()));
    let cancellation = kagari_common::cancellation::CancellationToken::default();
    let options = ExecutionOptions {
        cancellation: cancellation.clone(),
        ..Default::default()
    };
    let session = rt.begin_execution(&loaded, options).unwrap();
    let value = rt
        .iter_operation(
            &loaded,
            &root.value(rt.gc()).unwrap(),
            &Ty::Array(Box::new(item), CollectionAccess::Mutable),
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
            .iter_operation(&loaded, &iter.value(rt.gc()).unwrap(), &ty, IterOp::Next)
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
        rt.iter_operation(&loaded, &iter.value(rt.gc()).unwrap(), &ty, IterOp::Next)
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
            .iter_operation(
                &other_loaded,
                &iter.value(rt.gc()).unwrap(),
                &ty,
                IterOp::Next
            )
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
        r#"use std::iter::{CollectionCursor};

        struct Values { val items: Vec<i32> }
        impl Iterable for Values {
            type Item = i32;
            type Iter = CollectionCursor<i32>;
            fn iter(self) -> Self::Iter { self.items.iter() }
        }
        fn sum<I: Iterable<Item = i32>>(source: I) -> i32 {
            var total = 0;
            for item in source { total += item; }
            total
        }
        fn main() -> i32 {
            val values = Values { items: [20, 22] };
            val items: CollectionCursor<i32> = values.iter();
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
    if first.next() != Some(20) { return 0; }
    if alias.next() != Some(22) { return 0; }
    if second.next() != Some(20) { return 0; }
    if alias.next() != None { return 0; }
    if second.next() != Some(22) { return 0; }
    if second.next() != None { return 0; }
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
                )
                .is_err(),
            "{source}"
        );
    }
}

#[test]
fn malformed_native_iter_operations_are_rejected_before_execution() {
    use kagari_bytecode::instruction::{BytecodeInstruction, CallTarget};
    use kagari_types::{scalar::BuiltinType, ty::Ty};
    let artifact = KagariEngine::default()
        .compile_to_artifact(
            SourceFile::new(
                "iter-wire.kgr",
                "fn main()->i32 {var total=0;for x in [20,22]{total+=x;}total}",
            ),
            Default::default(),
        )
        .unwrap();
    let root = artifact.program.root.index();
    let import = artifact.program.modules[root]
        .native_imports
        .iter()
        .position(|import| {
            import
                .binding
                .path
                .last()
                .is_some_and(|part| part.name == "$foundation_list_iter")
        })
        .unwrap();
    for corrupt in 0..3 {
        let mut program = artifact.program.clone();
        match corrupt {
            0 => {
                program.modules[root].native_imports[import]
                    .signature
                    .params[0] = Ty::Builtin(BuiltinType::I32)
            }
            1 => {
                let instruction = program.modules[root]
                    .functions
                    .iter_mut()
                    .flat_map(|function| &mut function.instructions)
                    .find(|instruction| {
                        matches!(instruction, BytecodeInstruction::Call {
                        callee: CallTarget::Native(id), ..
                    } if id.index() == import)
                    })
                    .unwrap();
                let BytecodeInstruction::Call { args, .. } = instruction else {
                    unreachable!()
                };
                args.clear();
            }
            _ => {
                program.modules[root].native_imports[import]
                    .signature
                    .params[0] = Ty::Array(
                    Box::new(Ty::Enum(kagari_types::ty::NominalTy {
                        declaration: kagari_types::language::binding::option_declaration(),
                        arguments: vec![],
                        associated_types: Default::default(),
                    })),
                    CollectionAccess::Mutable,
                )
            }
        }
        assert!(verify_program(&program).is_err());
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
fn main()->i32{val calls=Calls{source:0,into:0,next:0};var total=0;for x in make(calls){total+=x;if total==42 {break;}}if calls.source != 1 { return 0; }if calls.into != 1 { return 0; }if calls.next != 2 { return 0; }total}
"#,
    );
}

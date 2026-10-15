use crate::test_support::cancellation as support;
use crate::{
    BytecodeArtifact,
    context::{ExecutionContext, JitPolicy},
    engine::{EngineConfig, KagariEngine},
    program::PreparedProgram,
};
use kagari_bytecode::{
    instruction::{BytecodeInstruction, CallTarget},
    program::{identity, verify_program},
};
use kagari_codegen_cranelift::CraneliftBackend;
use kagari_common::cancellation::CancellationToken;
use kagari_common::identity::map::DefinitionContext;
use kagari_contract::operations::IterOp;
use kagari_runtime::{
    module::entry::ProgramEntry, native::conversion::context::ConversionContext,
    session::ExecutionOptions, value::Value,
};
use kagari_source::source::SourceFile;
use kagari_types::{
    language::binding::option_declaration,
    scalar::BuiltinType,
    ty::{NominalTy, Ty},
};

#[test]
fn lazy_adapters_do_not_prefetch_or_fuse_resumable_sources() {
    execute(
        r#"
struct Pulse { var step: i32 }
impl Iterator for Pulse {
    type Item=i32;
    fn next(self)->Option<i32> {
        self.step+=1;
        if self.step==1 { Some(1) } else if self.step==3 { Some(3) } else { None }
    }
}
fn main()->i32 {
    val source=Pulse { step:0 };
    val zero=source.take(0usize);
    if zero.next()!=None || source.step!=0 { return 0; }
    val mapped=source.map(|x|x+39);
    if mapped.next()!=Some(40) || mapped.next()!=None { return 0; }
    val resumed=mapped.next();
    val filtered=Pulse {step:0}.filter(|x|x==3);
    if filtered.next()!=None || filtered.next()!=Some(3) { return 0; }
    val limited_source=Pulse {step:0};
    val limited=limited_source.take(1usize);
    val alias=limited;
    if limited.next()!=Some(1) || alias.next()!=None || limited_source.step!=1 { return 0; }
    match resumed { Some(x)=>x, None=>0 }
}
"#,
    );
}

#[test]
fn iterable_defaults_chain_on_containers_generic_custom_and_interface_receivers() {
    execute(
        r#"
struct Cursor { var position: i32 }
impl Iterator for Cursor {
    type Item = i32;
    fn next(self) -> Option<i32> {
        self.position += 1;
        if self.position <= 3 { Some(self.position) } else { None }
    }
}
struct Values {}
impl Iterable for Values {
    type Item = i32;
    type Iter = Cursor;
    fn iter(self) -> Cursor { Cursor { position: 0 } }
}
struct Override {}
impl Iterable for Override {
    type Item=i32;
    type Iter=Cursor;
    fn iter(self)->Cursor { Cursor {position:0} }
    fn map<U>(self, mapper:fn(i32)->U)->core::iter::MapIterator<i32,U> {
        Vec::from([10]).map(mapper)
    }
}
fn total<I: Iterable<Item=i32>>(values: I) -> i32 {
    values.map(|x| x * 2).filter(|x| x > 2).take(1usize).sum()
}
fn main() -> i32 {
    val custom=Override {};
    val overridden:Iterable<Item=i32,Iter=Cursor> = custom;
    if total(custom)!=20 || total(overridden)!=20 { return 0; }
    val values: Iterable<Item=i32,Iter=Cursor> = Values {};
    val cursor: Iterator<Item=i32> = Cursor { position: 0 };
    val mapped = cursor.map(|x| x + 10);
    val alias = mapped;
    val first = match mapped.next() { Some(x) => x, None => 0 };
    val second = match alias.next() { Some(x) => x, None => 0 };
    total(Vec::from([1,2,3])) + total([1,2,3]) + total(1..4) + total(Values {}) + total(values) + total(Cursor { position: 0 }) + first + second - 5
}
"#,
    );
}

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
            BytecodeArtifact::from_bytes(
                &artifact.records().to_bytes(artifact.definitions()).unwrap(),
            )
            .unwrap()
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
            let mut backend = CraneliftBackend::for_host().unwrap();
            let prepared = runtime
                .prepare_native(
                    &loaded_program,
                    &loaded,
                    ProgramEntry::Main,
                    &mut backend,
                    &context.cancellation,
                )
                .unwrap();
            runtime.execute_prepared(&loaded, ProgramEntry::Main, &[], &context, &prepared)
        } else {
            runtime.execute(&loaded, ProgramEntry::Main, &[], &context)
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
    val a=Vec::from([20,22]); val iter:CollectionCursor<i32> =a.iter();
    if first(iter) != 20 { return 0; }
    if first(iter) != 22 { return 0; }
    if first(iter) != 0 { return 0; }
    a.push(5);
    val scores:HashMap<String,i32> =HashMap::new();scores.insert("a",20);scores.insert("b",22);
    var total=0;for (key,value) in scores {total+=value;}
    if total != 42 { return 0; }
    val values:HashSet<i32> =HashSet::new();values.insert(20);values.insert(22);
    if sum(values) != 42 { return 0; }
    sum(Vec::from([20,22]))
}
"#,
    );
}

#[test]
fn breaking_native_loops_allows_resuming_unchanged_sources() {
    execute(
        r#"
fn main()->i32 {
    val a=Vec::from([20,22]); val iter=a.iter();var total=0;
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
fn returning_from_native_loop_does_not_block_caller_mutation() {
    execute(
        r#"
fn head(values:Vec<i32>)->i32 {for x in values {return x;}0}
fn main()->i32 {val a=Vec::from([20]);val b=head(a);a.push(22);b+a[1]}
"#,
    );
}

#[test]
fn native_iterators_validate_revisions_and_survive_gc_and_failure() {
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let engine = KagariEngine::new(config);
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "iterator-resources.kgr",
                r#"
pub fn fail()->i32{val a=Vec::from([20]);for x in a {a.push(1);}0}
pub fn nested()->i32{val a=Vec::from([20]);for x in a {for y in a {break;}a.push(1);}0}
pub fn manual()->i32{val a=Vec::from([20]);val c=a.iter();c.next();a.push(1);c.next();0}
pub fn trap()->i32{val a=Vec::from([20]);for x in a {return x/0;}0}
pub fn exhaust()->i32{val a=Vec::from([20]);for x in a {while true {}}0}
"#,
            ),
            Default::default(),
        )
        .unwrap();
    let artifact =
        BytecodeArtifact::from_bytes(&artifact.records().to_bytes(artifact.definitions()).unwrap())
            .unwrap();
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
        assert!(
            !format!("{error:?}").contains("MissingFunction"),
            "{entry}: {error:?}"
        );
        drop(cancellation);
        assert!(!format!("{error:?}").contains("UnsupportedExecution"));
        assert!(!runtime.runtime().is_quarantined());
        assert_eq!(runtime.runtime().gc().active_roots(), 0, "{entry}");
    }
    let rt = runtime.runtime();
    let root = ConversionContext::new(rt, &loaded)
        .unwrap()
        .encode(vec![20i32, 22])
        .unwrap();
    let Value::GcHandle(array) = root.value(rt.gc()).unwrap() else {
        panic!("Vec")
    };
    let vector_type = ConversionContext::new(rt, &loaded)
        .unwrap()
        .type_for::<Vec<i32>>()
        .unwrap();
    let item = Ty::Builtin(BuiltinType::I32);
    let ty = Ty::Iter(Box::new(item.clone()));
    let cancellation = CancellationToken::default();
    let options = ExecutionOptions {
        cancellation: cancellation.clone(),
        ..Default::default()
    };
    let session = rt.begin_execution(&loaded, options).unwrap();
    let value = rt
        .iter_operation(
            &loaded,
            &root.value(rt.gc()).unwrap(),
            vector_type.ty(),
            IterOp::New,
        )
        .unwrap();
    let iter = rt.root_value(value).unwrap();
    rt.gc().sequence_set(array, 0, Value::I32(20)).unwrap();
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
    // Structural edits commit between calls; the old cursor rejects its next read.
    rt.gc().sequence_push(array, Value::I32(1)).unwrap();
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
        "fn main(){[1].chain([true]);}",
        "fn main(){[1].zip(42);}",
        "fn main(){[1].filter_map(|x|x);}",
        "fn main(){[1].for_each(|x|x);}",
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
            val values = Values { items: Vec::from([20, 22]) };
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
    val values=Vec::from([20,22]); val first=values.iter(); val second=values.iter();
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
    let artifact = KagariEngine::default()
        .compile_to_artifact(
            SourceFile::new(
                "iter-wire.kgr",
                "fn main()->i32 {var total=0;for x in [20,22]{total+=x;}total}",
            ),
            Default::default(),
        )
        .unwrap();
    let context = DefinitionContext::new().unwrap();
    let artifact = artifact.import_into(&context, &Default::default()).unwrap();
    let table = identity::collect(
        &artifact
            .records()
            .program
            .modules
            .iter()
            .collect::<Vec<_>>(),
        &context,
    )
    .unwrap();
    verify_program(&artifact.records().program, &table).unwrap();
    let root = artifact.records().program.root.index();
    let import = artifact.records().program.modules[root]
        .native_imports
        .iter()
        .position(|import| {
            artifact
                .definitions()
                .resolve(import.binding)
                .unwrap()
                .last()
                .is_some_and(|part| part.name == "$foundation_array_iter")
        })
        .unwrap();
    for corrupt in 0..3 {
        let mut program = artifact.records().program.clone();
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
                    .params[0] = Ty::Array(Box::new(Ty::Enum(NominalTy {
                    declaration: artifact
                        .definitions()
                        .lookup(&option_declaration())
                        .unwrap(),
                    arguments: vec![],
                    associated_types: Default::default(),
                })))
            }
        }
        assert!(verify_program(&program, &context.snapshot()).is_err());
        assert!(BytecodeArtifact::from_program(program, Default::default(), &context).is_err());
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

#[test]
fn paired_adapters_select_both_sources_and_preserve_laziness_and_resumption() {
    execute(
        r#"
use std::collections::List;
struct Source { val log:Vec<i32>, val id:i32 }
struct Cursor { val log:Vec<i32>, val id:i32, var step:i32 }
fn make(log:Vec<i32>,id:i32)->Source { log.push(id); Source {log,id} }
impl Iterable for Source {
    type Item=i32; type Iter=Cursor;
    fn iter(self)->Cursor { self.log.push(self.id+10); Cursor {log:self.log,id:self.id,step:0} }
}
impl Iterator for Cursor {
    type Item=i32;
    fn next(self)->Option<i32> {
        self.step+=1; self.log.push(self.id+20);
        if self.step==1 || self.step==3 {Some(self.step)} else {None}
    }
}
fn pairs<L:Iterable<Item=i32>,R:Iterable>(left:L,right:R)->usize {
    left.zip(right).enumerate().count()
}
fn main()->i32 {
    val log=Vec::new();
    val zipped=make(log,1).zip(make(log,2));
    if log.len()!=4usize || log[0]!=1 || log[1]!=2 || log[2]!=11 || log[3]!=12 {return 0;}
    val alias=zipped;
    match alias.next() {Some(pair)=>{if pair[0]!=1 || pair[1]!=1 {return 0;}},None=>{return 0;}};
    if log[4]!=21 || log[5]!=22 {return 0;}
    if zipped.next()!=None || log.len()!=7usize {return 0;}
    if alias.next()!=None || log.len()!=9usize {return 0;}
    val pulse=Cursor {log:Vec::new(),id:0,step:0};
    val mapped=pulse.filter_map(|x|if x==3 {Some(Vec::from([42]))} else {None});
    if mapped.next()!=None {return 0;}
    match mapped.next() {Some(value)=>{if value[0]!=42 {return 0;}},None=>{return 0;}};
    val first=Cursor {log:Vec::new(),id:0,step:0};
    val last=Cursor {log:Vec::new(),id:0,step:0};
    val chained=first.chain(last);
    if chained.next()!=Some(1) || chained.next()!=Some(1) || chained.next()!=None || chained.next()!=Some(3) || first.step!=2 {return 0;}
    val skipped=Cursor {log:Vec::new(),id:0,step:0}.skip(2usize);
    if skipped.next()!=None || skipped.next()!=None {return 0;}
    val counted:Iterable<Item=i32,Iter=Cursor> = Source {log:Vec::new(),id:0};
    val erased:Iterator<Item=String> = ["a","b"].iter();
    if pairs(counted,erased)!=1usize {return 0;}
    val list:List<i32> = Vec::from([10,20]);
    val output:Vec<(usize,(i32,String))> = list.zip(["a","b"]).enumerate().collect();
    if output[0][0]!=0usize || output[1][0]!=1usize || output[1][1][0]!=20 || output[1][1][1]!="b" {return 0;}
    42
}
"#,
    );
}

#[test]
fn short_circuit_terminals_share_progress_without_blocking_source_writes() {
    execute(
        r#"
fn main()->i32 {
    val left=Vec::from([1,2,3]); val right=Vec::from([10,20,30]); val calls=Vec::from([0]);
    val pipeline:Iterator<Item=(i32,i32)> = left.map(|x|x).zip(right.filter(|x|true));
    if !pipeline.any(|pair| {calls[0]+=1;pair[0]==2}) || calls[0]!=2 {return 0;}
    match pipeline.find(|pair|true) {Some(pair)=>{if pair[0]!=3 || pair[1]!=30 {return 0;}},None=>{return 0;}};
    left.push(4);right.push(40);
    val values=Vec::from([1,2,3,4]);
    val cursor=values.skip(1usize);val alias=cursor;
    if cursor.all(|x| {calls[0]+=1;x<3}) || calls[0]!=4 {return 0;}
    if alias.next()!=Some(4) {return 0;}
    for item in alias {break;}
    values.push(5);
    val empty:[i32]=[];
    if empty.any(|x|true) || !empty.all(|x|false) || empty.find(|x|true)!=None || empty.count()!=0usize {return 0;}
    val output=Vec::new();
    val unit=values.enumerate().for_each(|pair| {output.push(pair[1]);});
    if unit!=() || output.len()!=5usize || output[4]!=5 {return 0;}
    values.push(6);
    42
}
"#,
    );
}

#[test]
fn lazy_cursors_allow_edits_when_no_further_source_read_is_needed() {
    execute(
        r#"
fn main()->i32 {
    val values=Vec::from([1,2,3]);
    val unused=values.iter();
    values.push(4);
    val zero=values.take(0usize);
    if zero.next()!=None {return 0;}
    values.push(5);
    val one=values.take(1usize);
    if one.next()!=Some(1) {return 0;}
    values.push(6);
    if one.next()!=None {return 0;}
    val empty:Vec<i32> =Vec::new();
    val paired=empty.zip(values);
    if paired.next()!=None {return 0;}
    values.push(7);
    val short=values.iter();
    if !short.any(|x| {values.push(8);true}) {return 0;}
    for x in values {values.push(9);break;}
    val fresh=values.iter();
    values[0]=42;
    if fresh.next()!=Some(42) || values.len()!=9usize {return 0;}
    42
}
"#,
    );
}

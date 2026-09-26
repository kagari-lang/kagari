use kagari_common::SourceFile;
use kagari_embed::{BytecodeArtifact, ExecutionContext, KagariEngine};
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
        let loaded = runtime.load_program(artifact, Default::default()).unwrap();
        let result = if jit {
            let mut backend = kagari_jit_cranelift::CraneliftBackend::for_host().unwrap();
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
fn custom_iterators_and_iterables_use_static_calls() {
    execute(
        r#"
struct Counter {var value:i32,val end:i32}
impl Iterator for Counter {type Item=i32;fn next(self)->Option<i32>{if self.value>=self.end {None}else{val value=self.value;self.value+=1;Some(value)}}}
struct Range {val start:i32,val end:i32}
impl IntoIterator for Range {type Item=i32;type IntoIter=Counter;fn into_iter(self)->Counter{Counter{value:self.start,end:self.end}}}
fn sum<C:IntoIterator<Item=i32>>(values:C)->i32 {var total=0;for value in values {if value==0 {continue;}total+=value;if total==42 {break;}}total}
fn main()->i32 {val a=Range{start:0,end:7};val b=Counter{value:0,end:7};sum(a)+sum(b)}
"#,
    );
}

#[test]
fn native_iterators_support_generic_bounds_and_unicode() {
    execute(
        r#"
fn sum<C:IntoIterator<Item=i32>>(values:C)->i32 {var total=0;for value in values {total+=value;}total}
fn first<I:Iterator<Item=i32>>(values:I)->i32 {match values.next(){Some(x)=>x,None=>0}}
fn main()->i32 {
    val a=[20,22]; val cursor:Cursor<i32> =a.into_iter();
    std::debug::assert_eq(first(cursor),20,"next");
    std::debug::assert_eq(first(cursor),22,"shared cursor");
    std::debug::assert_eq(first(cursor),0,"exhausted");
    a.push(5);
    val scores:Map<String,i32> =std::map::new();scores.insert("a",20);scores.insert("b",22);
    var total=0;for (key,value) in scores {total+=value;}
    std::debug::assert_eq(total,42,"map");
    val values:Set<i32> =std::set::new();values.insert(20);values.insert(22);
    std::debug::assert_eq(sum(values),42,"set");
    var count=0;for ch in "中😀".into_iter(){count+=1;}
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
    val a=[20,22]; val cursor=a.into_iter();var total=0;
    for value in cursor {total+=value;break;}
    for value in cursor {total+=value;}
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
fn head(values:[i32])->i32 {for x in values {return x;}0}
fn main()->i32 {val a=[20];val b=head(a);a.push(22);b+a[1]}
"#,
    );
}

#[test]
fn native_guards_release_on_failure_and_cursor_handles_survive_gc() {
    use kagari_ir::module::{
        abi::{AbiType, BuiltinType},
        instruction::CursorOp,
    };
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
fn manual()->i32{val a=[20];val c=a.into_iter();c.next();a.push(1);0}
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
    let loaded = runtime
        .load_program(artifact.clone(), Default::default())
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
    let ty = AbiType::Cursor(Box::new(item.clone()));
    let session = rt.begin_execution(&loaded, Default::default()).unwrap();
    let value = rt
        .cursor_operation(
            &loaded,
            &root.value(),
            &AbiType::Array(Box::new(item)),
            CursorOp::New,
        )
        .unwrap();
    let cursor = rt.root_value(value.clone()).unwrap();
    assert!(rt.gc().array_push(array, Value::I32(1)).is_err());
    drop(session);
    rt.collect_garbage().unwrap();
    for expected in [20, 22] {
        let session = rt.begin_execution(&loaded, Default::default()).unwrap();
        let Value::Enum(id) = rt
            .cursor_operation(&loaded, &cursor.value(), &ty, CursorOp::Next)
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
    // Between root calls guards are released; resumed cursors reject a changed structure.
    rt.gc().array_push(array, Value::I32(1)).unwrap();
    let session = rt.begin_execution(&loaded, Default::default()).unwrap();
    assert!(
        rt.cursor_operation(&loaded, &cursor.value(), &ty, CursorOp::Next)
            .is_err()
    );
    drop(session);
    let mut other = engine.runtime(context);
    let other_loaded = other.load_program(artifact, Default::default()).unwrap();
    let session = other
        .runtime()
        .begin_execution(&other_loaded, Default::default())
        .unwrap();
    assert!(
        other
            .runtime()
            .cursor_operation(&other_loaded, &cursor.value(), &ty, CursorOp::Next)
            .is_err()
    );
    drop(session);
    drop(cursor);
    rt.collect_garbage().unwrap();
    let session = rt.begin_execution(&loaded, Default::default()).unwrap();
    assert!(
        rt.cursor_operation(&loaded, &value, &ty, CursorOp::Next)
            .is_err()
    );
    drop(session);
}

#[test]
fn invalid_iterator_outputs_and_overrides_are_diagnostics() {
    for source in [
        "struct C{} impl Iterator for C {type Item=i32;fn next(self)->i32 {42}}",
        "struct C{} impl IntoIterator for C {type Item=i32;type IntoIter=i32;fn into_iter(self)->i32 {42}}",
        "struct C{} impl Iterator for C {type Item=i32;fn next(self)->Option<i32>{None}} impl IntoIterator for C {type Item=i32;type IntoIter=C;fn into_iter(self)->C {self}}",
        "struct C{} impl Iterator for C {type Item=String;fn next(self)->Option<String>{None}} struct R{} impl IntoIterator for R {type Item=i32;type IntoIter=C;fn into_iter(self)->C{C{}}}",
        "fn take<T:IntoIterator<Item=String>>(x:T){} fn main(){take([42]);}",
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
fn generic_iterator_bound_also_supplies_for_and_nested_associated_outputs() {
    execute(
        r#"
struct Counter{var value:i32}
impl Iterator for Counter{type Item=i32;fn next(self)->Option<i32>{if self.value>0 {val n=self.value;self.value=0;Some(n)}else{None}}}
fn consume<T:Iterator<Item=i32>>(it:T)->i32{var total=0;for value in it {total+=value;}total}
struct Wrap<T>{val inner:T}
impl<T:Iterator<Item=i32>> IntoIterator for Wrap<T>{type Item=i32;type IntoIter=T;fn into_iter(self)->T{self.inner}}
fn main()->i32 {val a=Wrap{inner:Counter{value:21}};var total=consume(Counter{value:21});for value in a {total+=value;}total}
"#,
    );
}

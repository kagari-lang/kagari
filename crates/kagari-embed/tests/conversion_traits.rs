use kagari_common::source::SourceFile;
use kagari_embed::{
    BytecodeArtifact,
    context::{ExecutionContext, JitPolicy},
    engine::{EngineConfig, KagariEngine},
    program::PreparedProgram,
};

use kagari_runtime::value::Value;

fn execute(source: &str) {
    let mut config = EngineConfig::default();
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
            JitPolicy::Enabled
        } else {
            JitPolicy::Disabled
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
fn explicit_and_generic_from_conversions() {
    execute(
        r#"
struct Count { val value:i32 }
impl From<i32> for Count { fn from(value:i32)->Self { Count{value} } }
fn convert<S,D:From<S>>(value:S)->Result<i32,D> { val error:Result<i32,S> = Err(value); Ok(error?) }
fn main()->i32 {
    val a:Result<i32,Count> = convert(10); val b:Result<i32,Count> = convert(11);
    match a {Err(x)=>match b {Err(y)=>x.value+y.value+21,Ok(_)=>0},Ok(_)=>0}
}
"#,
    );
}

#[test]
fn conversions_can_be_owned_by_the_source_and_preserve_identity() {
    execute(
        r#"
struct Count {val value:i32}
impl From<Count> for i32 {fn from(value:Count)->i32 {value.value}}
fn identity(value:Count)->Result<i32,Count> {val x:Result<i32,Count> = Err(value);Ok(x?)}
fn convert(value:Count)->Result<i32,i32> {val x:Result<i32,Count> = Err(value);Ok(x?)}
fn main()->i32 {val a=Count{value:42};match identity(a){Err(b)=>if a===b {match convert(b){Err(n)=>n,Ok(_)=>0}}else{0},Ok(_)=>0}}
"#,
    );
}
#[test]
fn invalid_conversion_implementations_are_diagnostics() {
    for source in [
        "struct X{} impl From<X> for X {fn from(value:X)->X {value}} fn main(){}",
        "struct X{} impl<T> From<X> for T {fn from(value:X)->T {loop{}}} fn main(){}",
        r#"impl From<i32> for String {fn from(value:i32)->String {"x"}} fn main(){}"#,
        "struct X{} impl From<i32> for X {fn from(self,value:i32)->Self {self}} fn main(){}",
        "struct X{} fn main()->X {X::from(1)}",
        "struct X{} impl From<i32> for X {fn from(v:i32)->X {X{}}} fn main(){X{}.from(1);}",
    ] {
        assert!(
            KagariEngine::default()
                .compile_to_artifact(
                    SourceFile::new("bad.kgr", source),
                    Default::default(),
                    Default::default()
                )
                .is_err(),
            "{source}"
        );
    }
}

#[test]
fn aliased_and_generic_from_bounds_evaluate_once() {
    execute(
        r#"
use core::language::From as Convert;
struct Count{val value:i32}
impl Convert<i32> for Count{fn from(value:i32)->Self{Count{value}}}
struct Calls{var count:i32}
fn source(c:Calls)->i32{c.count+=1;21}
fn convert<S,D:Convert<S>>(value:S)->Result<i32,D> {val x:Result<i32,S> = Err(value);Ok(x?)}
fn main()->i32 {
    val calls=Calls{count:0};
    val a:Result<i32,Count> = convert(source(calls));
    val b:Result<i32,Count> = convert(source(calls));
    if calls.count != 2 { return 0; }
    match a {Err(x)=>match b {Err(y)=>x.value+y.value,Ok(_)=>0},Ok(_)=>0}
}
"#,
    );
}

#[test]
fn unrelated_into_method_keeps_ordinary_trait_dispatch() {
    execute(
        r#"
trait Local{fn into(self)->i32;}
struct X{}
impl Local for X{fn into(self)->i32{42}}
fn main()->i32{X{}.into()}
"#,
    );
}

#[test]
fn imported_generic_conversions_and_iterators_link_to_their_defining_module() {
    use kagari_common::{
        identity::{ModuleIdentity, PackageId},
        source_database::SourceLayer,
    };
    let engine = KagariEngine::default();
    let mut root = None;
    for (name, source) in [
        (
            "model",
            r#"
pub struct Wrapper<T>{pub val value:T,var consumed:bool}
impl<T> From<T> for Wrapper<T>{fn from(value:T)->Self{Wrapper{value,consumed:false}}}
impl<T> Iterator for Wrapper<T>{type Item=T;fn next(self)->Option<T>{if self.consumed {None}else{self.consumed=true;Some(self.value)}}}
"#,
        ),
        (
            "root",
            r#"
use pkg::model::Wrapper;
fn total<I:Iterable<Item=i32>>(values:I)->i32{var n=0;for x in values{n+=x;}n}
fn propagate()->Result<i32,Wrapper<i32>> {val x:Result<i32,i32> = Err(42);Ok(x?)}
fn main()->i32 {match propagate(){Err(w)=>total(w),Ok(x)=>x}}
"#,
        ),
    ] {
        let path = format!("mem://{name}");
        engine
            .bind_module(
                &path,
                ModuleIdentity {
                    package: PackageId("pkg".into()),
                    path: vec![name.into()],
                },
            )
            .unwrap();
        let id = engine
            .set_source(&path, source.into(), SourceLayer::Base)
            .unwrap();
        if name == "root" {
            root = Some(id);
        }
    }
    let checked = engine
        .compile_snapshot(
            engine.source_snapshot(),
            root.unwrap(),
            Default::default(),
            &Default::default(),
        )
        .unwrap();
    let artifact = engine.emit_bytecode(&checked, Default::default()).unwrap();
    let artifact = BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded_program =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let loaded = runtime
        .load_program(&loaded_program, Default::default())
        .unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, "main", &[], &context)
            .unwrap()
            .return_value,
        Value::I32(42)
    );
}

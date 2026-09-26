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
fn explicit_and_derived_conversions() {
    execute(
        r#"
struct Count {val value:i32}
impl From<i32> for Count {fn from(value:i32)->Self {Count{value}}}
impl TryFrom<i32> for Count {type Error=String;fn try_from(value:i32)->Result<Self,String> {if value<0 {Err("negative")}else{Ok(Count{value})}}}
fn convert<S,D:From<S>>(value:S)->D {D::from(value)}
fn into<S:Into<D>,D>(value:S)->D {value.into()}
fn checked<S:TryInto<D,Error=String>,D>(value:S)->Result<D,String> {value.try_into()}
fn main()->i32 {
 val a=Count::from(10);val b:Count=convert(11);val c:Count=into(21);
 val ok:Result<Count,String> = checked(42);
 val bad=Count::try_from(-1);
 if bad.is_err() && ok.is_ok() {a.value+b.value+c.value}else{0}
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
fn main()->i32 {val a=Count{value:42};val b:Count=a.into();val result:i32=b.into();if a===b && i32::from(a)==42 {result}else{0}}
"#,
    );
}
#[test]
fn invalid_conversion_implementations_are_diagnostics() {
    for source in [
        "struct X{} impl Into<i32> for X {fn into(self)->i32 {1}} fn main(){}",
        "struct X{} impl TryInto<i32> for X {type Error=String;fn try_into(self)->Result<i32,String>{Ok(1)}} fn main(){}",
        "struct X{} impl From<X> for X {fn from(value:X)->X {value}} fn main(){}",
        "struct X{} impl<T> From<X> for T {fn from(value:X)->T {loop{}}} fn main(){}",
        r#"impl From<i32> for String {fn from(value:i32)->String {"x"}} fn main(){}"#,
        "struct X{} impl From<i32> for X {fn from(self,value:i32)->Self {self}} fn main(){}",
        "struct X{} impl TryFrom<i32> for X {fn try_from(value:i32)->Result<Self,String>{Ok(X{})}} fn main(){}",
        "struct X{} fn main()->X {X::from(1)}",
        "fn main(){val x=1.into();}",
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
fn qualified_calls_and_fallible_error_projections_evaluate_once() {
    execute(
        r#"
use std::convert::From as Convert;
struct Count{val value:i32}
impl Convert<i32> for Count{fn from(value:i32)->Self{Count{value}}}
impl TryFrom<i32> for Count{type Error=String;fn try_from(value:i32)->Result<Self,String>{if value<0 {Err("negative")}else{Ok(Count{value})}}}
struct Calls{var count:i32}
fn source(c:Calls)->i32{c.count+=1;21}
fn checked<S:TryInto<D>,D>(value:S)->Result<D,<S as TryInto<D>>::Error>{value.try_into()}
fn main()->i32{val calls=Calls{count:0};val a=<Count as Convert<i32>>::from(source(calls));val b:Result<Count,String> = checked(source(calls));std::debug::assert_eq(calls.count,2,"single evaluation");match b{Ok(n)=>a.value+n.value,Err(e)=>0}}
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
fn total<I:IntoIterator<Item=i32>>(values:I)->i32{var n=0;for x in values{n+=x;}n}
fn main()->i32 {val a:Wrapper<i32> = 42.into();total(a)}
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
    let loaded = runtime.load_program(artifact, Default::default()).unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, "main", &[], &context)
            .unwrap()
            .return_value,
        Value::I32(42)
    );
}

use kagari_embed::{
    BytecodeArtifact,
    context::{ExecutionContext, JitPolicy},
    engine::{EngineConfig, KagariEngine},
    program::PreparedProgram,
};
use kagari_source::source::SourceFile;

use kagari_runtime::value::Value;

fn execute(source: &str) {
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let engine = KagariEngine::builder().config(config).build().unwrap();
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
                .compile_to_artifact(SourceFile::new("bad.kgr", source), Default::default())
                .is_err(),
            "{source}"
        );
    }
}

#[test]
fn aliased_and_generic_from_bounds_evaluate_once() {
    execute(
        r#"
use core::convert::From as Convert;
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
    use {
        kagari_common::identity::{ModuleIdentity, PackageId},
        kagari_source::source_database::SourceLayer,
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
        .compile_snapshot(engine.source_snapshot(), root.unwrap(), &Default::default())
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

#[test]
fn reverse_conversions_call_the_destination_implementation() {
    execute(
        r#"
struct Count {val value:i32}
impl From<i32> for Count {fn from(value:i32)->Self {Count{value}}}
impl TryFrom<i32> for Count {
    type Error=String;
    fn try_from(value:i32)->Result<Self,String> {if value<0 {Err("negative")}else{Ok(Count{value})}}
}
fn convert<S:Into<D>,D>(value:S)->D {value.into()}
fn attempt<S:TryInto<D,Error=String>,D>(value:S)->Result<D,String> {value.try_into()}
fn main()->i32 {
    val a:Count=convert(20);
    val b:Result<Count,String> = attempt(22);
    match b {Ok(b)=>a.value+b.value,Err(_)=>0}
}
"#,
    );
}

#[test]
fn static_calls_share_ordinary_trait_dispatch() {
    execute(
        r#"use std::iter::{Product, Sum};
use std::str::{FromStr};

struct Count {val value:i32}
impl FromStr for Count {
    type Err=String;
    fn from_str(text:String)->Result<Self,String> {if text=="42" {Ok(Count{value:42})}else{Err(text)}}
}
impl Sum<i32> for Count {
    fn sum<I:Iterable<Item=i32>>(source:I)->Self {var value=0;for item in source {value=value+item;}Count{value}}
}
impl Product<i32> for Count {
    fn product<I:Iterable<Item=i32>>(source:I)->Self {var value=1;for item in source {value=value*item;}Count{value}}
}
impl FromIterator<i32> for Count {
    fn from_iter<I:Iterable<Item=i32>>(source:I)->Self {Self::sum(source)}
}
fn collect<T:FromIterator<i32>,I:Iterable<Item=i32>>(source:I)->T {T::from_iter(source)}
fn main()->i32 {
    val a:Count=collect([10,11]);val b=Count::product([3,7]);
    val parsed=<Count as FromStr<Err=String>>::from_str("42");
    match parsed {Ok(n)=>if n.value==a.value+b.value {n.value}else{0},Err(_)=>0}
}
"#,
    );
}

#[test]
fn foundation_construction_and_primitive_conversions() {
    execute(
        r#"use std::iter::{Sum};
use std::num::{TryFromIntError};

fn aggregate<T:Sum<i32>,I:Iterable<Item=i32>>(source:I)->T {T::sum(source)}
fn main()->i32 {
    val copied=Vec<i32>::from_iter([2,3,7]);
    val product=i32::product(copied);
    val total:i32=aggregate([20,22]);
    val parsed=i32::from_str("42");
    val narrow:Result<i8,TryFromIntError> = total.try_into();
    match parsed {Ok(n)=>match narrow {Ok(k)=>if product==n && i32::from(k)==n {n}else{0},Err(_)=>0},Err(_)=>0}
}
"#,
    );
}

#[test]
fn foundation_aggregation_calls_script_iterators_synchronously() {
    execute(
        r#"
struct Counter {var value:i32}
impl Iterator for Counter {
    type Item=i32;
    fn next(self)->Option<i32> {if self.value<22 {self.value=self.value+1;Some(self.value)}else{None}}
}
fn main()->i32 {
    val list=Vec<i32>::from_iter(Counter{value:19});
    val total=i32::sum(list);
    total-21
}
"#,
    );
}

#[test]
fn construction_contracts_reject_invalid_signatures_and_bounds() {
    for source in [
        "struct X{} impl Into<i32> for X {fn into(self)->i32 {1}} fn main(){}",
        "struct X{} impl TryInto<i32> for X {type Error=String;fn try_into(self)->Result<i32,String>{Ok(1)}} fn main(){}",
        "use std::str::{FromStr};\nstruct X{} impl FromStr for X {type Err=String;fn from_str(text:i32)->Result<X,String>{Ok(X{})}} fn main(){}",
        "use std::str::{FromStr};\nstruct X{} impl FromStr for X {type Err=String;fn from_str(text:String)->Result<X,String>{Ok(X{})}} fn main(){<X as FromStr<Err=i32>>::from_str(\"42\");}",
        "struct X{} impl TryFrom<i32> for X {type Error=String;fn try_from(value:i32)->X{X{}}} fn main(){}",
        "use std::iter::{Sum};\nstruct X{} impl Sum<i32> for X {fn sum<I:Iterable<Item=String>>(source:I)->X{X{}}} fn main(){}",
        "use std::iter::{Product};\nstruct X{} impl Product<i32> for X {fn product(source:i32)->X{X{}}} fn main(){}",
        "fn main(){val x=Vec<i32>::from_iter([true]);}",
        "fn main()->i32 {i32::sum([true])}",
        "fn main(){val x:Result<i32,String> = i32::try_from(1);}",
    ] {
        let engine = KagariEngine::builder().build().unwrap();
        assert!(
            engine
                .compile_to_artifact(
                    SourceFile::new("invalid-foundation.kgr", source),
                    Default::default()
                )
                .is_err(),
            "{source}"
        );
    }
}

#[test]
fn primitive_construction_preserves_errors_empty_identities_and_fresh_storage() {
    execute(
        r#"use std::convert::{Infallible};
use std::num::{ParseError, TryFromIntError};

fn main()->i32 {
    val source=[1,2];val copy=Vec<i32>::from_iter(source);copy.push(3);
    val empty_values:Vec<i32> = [];
    if source.len()!=2usize || i32::sum(empty_values)!=0 || i32::product(empty_values)!=1 {return 0;}
    val out=i8::try_from(128);val empty=i32::from_str("");val invalid=bool::from_str("TRUE");
    val narrow:Result<i8,Infallible> = i8::try_from(42i8);
    match out {Err(TryFromIntError::OutOfRange)=>match empty {
        Err(ParseError::Empty)=>match invalid {Err(ParseError::InvalidSyntax)=>match narrow {Ok(n)=>i32::from(n),Err(e)=>match e {}},_=>0},_=>0},_=>0}
}
"#,
    );
}

#[test]
fn native_numeric_aggregation_checks_declared_width_and_releases_roots() {
    let engine = KagariEngine::builder().build().unwrap();
    for source in [
        "fn main()->i8 {i8::sum([127i8,1i8])}",
        "fn main()->u8 {u8::product([128u8,2u8])}",
    ] {
        let artifact = engine
            .compile_to_artifact(SourceFile::new("overflow.kgr", source), Default::default())
            .unwrap();
        let program =
            PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap();
        let context = ExecutionContext::default();
        let mut runtime = engine.runtime(context.clone());
        let loaded = runtime.load_program(&program, Default::default()).unwrap();
        let error = runtime.execute(&loaded, "main", &[], &context).unwrap_err();
        assert!(
            format!("{error:?}").contains("integer overflow"),
            "{error:?}"
        );
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
    }
}

#[test]
fn native_aggregation_accepts_readonly_collection_interfaces() {
    execute(
        r#"use std::collections::{List};

fn main()->i32 {val values:List<i32> = [20,22]; i32::sum(values)}
"#,
    );
}

#[test]
fn native_construction_accepts_custom_iterable_implementations() {
    execute(
        r#"
struct Source {val values:Vec<i32>}
struct Cursor {val values:Vec<i32>,var index:usize}
impl Iterator for Cursor {
    type Item=i32;
    fn next(self)->Option<i32> {if self.index<self.values.len() {val item=self.values[self.index];self.index=self.index+1usize;Some(item)}else{None}}
}
impl Iterable for Source {
    type Item=i32;type Iter=Cursor;
    fn iter(self)->Cursor {Cursor{values:self.values,index:0usize}}
}
fn main()->i32 {i32::sum(Source{values:[20,22]})}
"#,
    );
}

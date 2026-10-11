mod support;
use kagari_bytecode::program::verify_program;
use kagari_embed::{
    BytecodeArtifact,
    context::{ExecutionContext, JitPolicy},
    engine::{EngineConfig, KagariEngine},
    error::EmbeddingError,
    program::PreparedProgram,
};
use kagari_runtime::value::Value;
use kagari_source::source::SourceFile;
use kagari_types::language as standard_traits;

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
fn ordering_protocols_and_builtin_comparisons_agree() {
    execute(
        r#"use std::cmp::{Ordering};

struct Rank {val value:i32}
impl PartialEq for Rank {fn eq(self,other:Self)->bool {self.value==other.value}}
impl Eq for Rank {}
impl PartialOrd for Rank {fn partial_cmp(self,other:Self)->Option<Ordering> {self.value.partial_cmp(other.value)}}
impl Ord for Rank {fn cmp(self,other:Self)->Ordering {self.value.cmp(other.value)}}
fn before<T:Ord>(a:T,b:T)->bool {a<b && a<=b && !(a>b) && !(a>=b)}
fn main()->i32 {
 val a=Rank{value:1};val b=Rank{value:2};
 if before(a,b) && a<=a && a>=a && before(1,2) && before("a","b") && a.cmp(b)==Ordering::Less && Ordering::Less<Ordering::Greater {42}else{0}
}
"#,
    );
}

#[test]
fn unordered_custom_comparisons_are_false_for_all_operators() {
    execute(
        r#"use std::cmp::{Ordering};

struct Unknown {}
impl PartialEq for Unknown {fn eq(self,other:Self)->bool {false}}
impl PartialOrd for Unknown {fn partial_cmp(self,other:Self)->Option<Ordering> {None}}
fn main()->i32 {val a=Unknown{}; if !(a<a) && !(a<=a) && !(a>a) && !(a>=a) && a.partial_cmp(a) == None {42}else{0}}
"#,
    );
}

#[test]
fn invalid_ordering_contracts_are_diagnostics() {
    for source in [
        "use std::cmp::{Ordering};\nfn main(){val a=Ordering::Equal?;}",
        "use std::cmp::{Ordering};\nfn main()->Ordering {Ordering::Equal?}",
        "fn needs<T:Ord>(v:T){} fn main(){needs(1.0);}",
        "use std::cmp::{Ordering};\nstruct X{} impl Ord for X {fn cmp(self,other:Self)->Ordering {Ordering::Equal}} fn main(){}",
        "struct X{} fn main()->bool {X{}<X{}}",
        "fn main()->bool {(1,2)<(2,3)}",
    ] {
        assert!(
            matches!(
                KagariEngine::default().compile_to_artifact(
                    SourceFile::new("bad-order.kgr", source),
                    Default::default()
                ),
                Err(EmbeddingError::Diagnostics { .. })
            ),
            "{source}"
        );
    }
}

#[test]
fn arithmetic_protocols_have_rhs_and_associated_output() {
    execute(
        r#"use std::ops::{Add, Div, Mul, Rem, Sub};

use core::ops::Add as Plus;
struct Vector {val x:i32}
impl Plus<Vector> for Vector {type Output=Vector;fn add(self,rhs:Vector)->Vector {Vector{x:self.x+rhs.x}}}
impl Mul<i32> for Vector {type Output=Vector;fn mul(self,rhs:i32)->Vector {Vector{x:self.x*rhs}}}
impl Sub<Vector> for Vector {type Output=i32;fn sub(self,rhs:Vector)->i32 {self.x-rhs.x}}
impl Div<i32> for Vector {type Output=i32;fn div(self,rhs:i32)->i32 {self.x/rhs}}
impl Rem<i32> for Vector {type Output=i32;fn rem(self,rhs:i32)->i32 {self.x%rhs}}
fn plus<T:Plus<T>>(a:T,b:T)->T::Output {a+b}
fn scaled<T:Mul<i32,Output=Vector>>(a:T)->Vector {a*2}
fn main()->i32 {
 val a=Vector{x:10};val b=Vector{x:11};
 if (a+b).x==a.add(b).x && plus(1,2)==3 && a-b == -1 && b/2==5 && b%2==1 {scaled(plus(a,b)).x}else{0}
}
"#,
    );
}

#[test]
fn generic_arithmetic_impls_forward_operator_bounds() {
    execute(
        r#"use std::ops::{Add};

struct Wrap<T>{val item:T}
impl<T:Add<T,Output=T>> Add<Wrap<T>> for Wrap<T> {type Output=Wrap<T>;fn add(self,rhs:Wrap<T>)->Wrap<T> {Wrap{item:self.item+rhs.item}}}
fn main()->i32 {(Wrap{item:20}+Wrap{item:22}).item}
"#,
    );
}

#[test]
fn same_named_application_trait_does_not_replace_the_language_role() {
    execute(
        r#"
use core::ops::Add as LanguageAdd;
trait Add<Rhs> { fn unrelated(self, rhs: Rhs) -> i32; }
struct Number { val value: i32 }
impl Add<Number> for Number { fn unrelated(self, rhs: Number) -> i32 { 0 } }
impl LanguageAdd<Number> for Number {
    type Output = i32;
    fn add(self, rhs: Number) -> i32 { self.value + rhs.value }
}
fn main() -> i32 { Number { value: 20 } + Number { value: 22 } }
"#,
    );
}

#[test]
fn unary_protocols_support_generic_and_different_output_types() {
    execute(
        r#"use std::ops::{Neg, Not};

struct Signed {val value:i32}
impl Neg for Signed {type Output=Signed;fn neg(self)->Signed {Signed{value:-self.value}}}
impl Not for Signed {type Output=bool;fn not(self)->bool {self.value==0}}
fn negative<T:Neg>(x:T)->T::Output {-x}
fn invert<T:Not>(x:T)->T::Output {!x}
fn main()->i32 {val x=Signed{value:-42};val zero=Signed{value:0};if invert(zero) && !invert(x) && negative(1)== -1 && invert(false) && (-x).value==x.neg().value {negative(x).value}else{0}}
"#,
    );
}

#[test]
fn readonly_index_returns_shared_objects_without_container_writeback() {
    execute(
        r#"use std::ops::{Index};

struct Item {var value:i32}
struct Bag {val items:Vec<Item>,var reads:i32}
impl Index<i32> for Bag {type Output=Item;fn index(self,rhs:i32)->Item {self.reads+=1;self.items[rhs as usize]}}
fn read<C:Index<i32>>(c:C,i:i32)->C::Output {c[i]}
fn main()->i32 {
 val item=Item{value:0};val bag=Bag{items:Vec::from([item]),reads:0};
 bag[0].value=20;bag[0].value+=22;
 if item.value==42 && bag.reads==2 && read(bag,0)===item && bag.index(0)===item && read([42],0)==42 {item.value}else{0}
}
"#,
    );
}

#[test]
fn index_does_not_grant_element_replacement_or_immutable_field_writes() {
    for tail in ["b[0]=Item{value:1};", "b[0].value=1;"] {
        let source = format!(
            "use std::ops::{{Index}};\nstruct Item {{val value:i32}} struct Bag {{val item:Item}} impl Index<i32> for Bag {{type Output=Item;fn index(self,rhs:i32)->Item {{self.item}}}} fn main() {{val b=Bag{{item:Item{{value:0}}}};{tail}}}"
        );
        assert!(
            matches!(
                KagariEngine::default().compile_to_artifact(
                    SourceFile::new("bad-index.kgr", source),
                    Default::default()
                ),
                Err(EmbeddingError::Diagnostics { .. })
            ),
            "{tail}"
        );
    }
}

#[test]
fn ordering_aliases_patterns_and_float_unordered_behavior() {
    execute(
        r#"
use core::cmp::Ordering as Order;
use core::cmp::Ordering::*;
fn rank(x:Order)->i32 {match x {Less=>0,Equal=>1,Greater=>2}}
fn main()->i32 {val nan=0.0/0.0; if nan.partial_cmp(nan) == None && !(nan<nan) && !(nan<=nan) && !(nan>nan) && !(nan>=nan) && rank(Order::Equal)==1 {42}else{0}}
"#,
    );
}

#[test]
fn operator_operands_and_index_getters_evaluate_once_in_order() {
    execute(
        r#"use std::ops::{Add, Index};

struct State {var steps:i32}
struct Item {var value:i32}
struct Bag {var item:Item,val state:State}
impl Index<i32> for Bag {type Output=Item;fn index(self,rhs:i32)->Item {self.state.steps=self.state.steps*10+3;self.item}}
fn receiver(b:Bag)->Bag {b.state.steps=b.state.steps*10+1;b}
fn index(s:State)->i32 {s.steps=s.steps*10+2;0}
fn rhs(b:Bag)->i32 {b.state.steps=b.state.steps*10+4;b.item=Item{value:99};42}
struct Number {val value:i32}
impl Add<Number> for Number {type Output=Number;fn add(self,rhs:Number)->Number {Number{value:self.value+rhs.value}}}
fn number(s:State,digit:i32)->Number {s.steps=s.steps*10+digit;Number{value:21}}
fn main()->i32 {
 val s=State{steps:0};val old=Item{value:0};val b=Bag{item:old,state:s};
 receiver(b)[index(s)].value=rhs(b);
 if !(s.steps==1234 && old.value==42 && b.item.value==99) { return 0; }
 s.steps=0;val sum=number(s,1)+number(s,2);
 if s.steps==12 {sum.value}else{0}
}
"#,
    );
}

#[test]
fn invalid_operator_signatures_bounds_and_writes_are_diagnostics() {
    for source in [
        "use std::ops::{Add};\nstruct X{} impl Add<X> for X {fn add(self,rhs:X)->X {self}} fn main(){}",
        "use std::ops::{Add};\nstruct X{} impl Add<X> for X {type Output=i32;fn add(self,rhs:X)->bool {true}} fn main(){}",
        "use std::ops::{Add};\nstruct X{} impl Add<X> for X {type Output=X;fn add(self,rhs:X)->X {self}} fn main(){var a=X{};a+=X{};}",
        "use std::ops::{Add};\nfn f<T:Add<i32,Output=bool>>(x:T){} fn main(){f(1);}",
        "fn f<T:Ord>(x:T){} enum E{A} fn main(){f(E::A);}",
        "fn f<T:Ord>(x:T){} fn main(){f((1,2));}",
        "use std::ops::{Add};\nimpl Add<i32> for i32 {type Output=i32;fn add(self,rhs:i32)->i32 {0}} fn main(){}",
        "use std::ops::{Neg};\nstruct X{} impl Neg for X {type Output=i32;fn neg(self)->i32 {42}} fn main(){val x=X{};val b=x && x;}",
    ] {
        let result = KagariEngine::default().compile_to_artifact(
            SourceFile::new("bad-operator.kgr", source),
            Default::default(),
        );
        assert!(
            matches!(result, Err(EmbeddingError::Diagnostics { .. })),
            "{source}: {result:?}"
        );
    }
}

#[test]
fn array_index_methods_use_the_actual_integer_argument() {
    execute("fn main()->i32 {val a=[42];val zero=a.len()-a.len();a.index(zero)}");
}

#[test]
fn imported_generic_operators_keep_the_defining_implementation() {
    use {
        kagari_common::identity::{ModuleIdentity, PackageId},
        kagari_source::source_database::SourceLayer,
    };
    for downstream_override in [false, true] {
        let engine = KagariEngine::default();
        let model = r#"use std::ops::{Add, Index};

pub struct Box<T> {pub val value:T}
impl<T:Add<T,Output=T>> Add<T> for Box<T> {type Output=Box<T>;fn add(self,rhs:T)->Box<T> {Box{value:self.value+rhs}}}
impl<T> Index<i32> for Box<T> {type Output=T;fn index(self,rhs:i32)->T {self.value}}
fn plus<T:Add<T,Output=T>>(a:Box<T>,b:T)->Box<T> {a+b}
pub fn add(a:Box<i32>,b:i32)->Box<i32> {plus(a,b)}
"#;
        let root_source = if downstream_override {
            "use std::ops::{Neg};\nuse pkg::model::Box; impl Neg for Box<i32> {type Output=i32;fn neg(self)->i32 {0}} fn main()->i32 {42}"
        } else {
            "use std::ops::{Index};\nuse pkg::model::{Box,add}; fn read<T:Index<i32,Output=i32>>(v:T)->i32 {v[0]} fn main()->i32 {read(add(Box{value:20},1)+21)}"
        };
        let mut root = None;
        for (name, source) in [("model", model), ("root", root_source)] {
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
        let checked =
            engine.compile_snapshot(engine.source_snapshot(), root.unwrap(), &Default::default());
        if downstream_override {
            assert!(checked.is_err());
            continue;
        }
        let artifact = engine
            .emit_bytecode(&checked.unwrap(), Default::default())
            .unwrap();
        let artifact = BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
        let context = ExecutionContext::default();
        let mut runtime = engine.runtime(context.clone());
        let loaded_program =
            PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap();
        let loaded = runtime
            .load_program(&loaded_program, Default::default())
            .unwrap();
        assert_eq!(
            runtime
                .execute(&loaded, "main", &[], &context)
                .unwrap()
                .return_value
                .value(runtime.runtime().gc())
                .expect("retained execution result"),
            Value::I32(42)
        );
    }
}

#[test]
fn portable_operator_contracts_reject_wrong_inputs_and_outputs() {
    use {
        kagari_contract::types::PublicItem,
        kagari_types::{language::Protocol, ty::Ty},
    };
    let artifact = KagariEngine::default()
        .compile_to_artifact(
            SourceFile::new(
                "operator-wire.kgr",
                r#"use std::ops::{Add};

struct Number{val value:i32}
impl Add<i32> for Number {type Output=i32;fn add(self,rhs:i32)->i32 {self.value+rhs}}
fn main()->i32 {Number{value:20}+22}
"#,
            ),
            Default::default(),
        )
        .unwrap();
    for corrupt_input in [false, true] {
        let mut program = artifact.program.clone();
        let module = &mut program.modules[program.root.index()];
        let table=module.public_items.iter_mut().find_map(|item| match item {
            PublicItem::InterfaceTable(table) if matches!(&table.trait_type,Ty::Trait(t) if t.declaration==standard_traits::identity(Protocol::Add))=>Some(table),
            _=>None,
        }).unwrap();
        let Ty::Trait(interface) = &mut table.trait_type else {
            unreachable!()
        };
        if corrupt_input {
            interface.arguments.clear();
        } else {
            interface.associated_types.clear();
        }
        assert!(verify_program(&program).is_err());
    }
}

#[test]
fn arithmetic_stays_direct_and_indexing_uses_its_checked_native_binding() {
    use kagari_bytecode::instruction::{BytecodeInstruction, CallTarget};
    let artifact = KagariEngine::default()
        .compile_to_artifact(
            SourceFile::new(
                "fast-ops.kgr",
                r#"
fn main()->i32 {val a=[40];val b=a[0]+4-2;if b>=42 && !false {-(-b)}else{0}}
"#,
            ),
            Default::default(),
        )
        .unwrap();
    let module = &artifact.program.modules[artifact.program.root.index()];
    let main = module
        .functions
        .iter()
        .find(|function| function.name == "main")
        .unwrap();
    let calls = main
        .instructions
        .iter()
        .filter_map(|instruction| match instruction {
            BytecodeInstruction::Call { callee, .. } => Some(callee),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(calls.len(), 1);
    let CallTarget::Native(index) = calls[0] else {
        panic!("native Index binding")
    };
    assert_eq!(
        module.native_imports[index.index()]
            .binding
            .path
            .last()
            .unwrap()
            .name,
        "$foundation_array_index"
    );
}

#[test]
fn operator_traps_and_cancellation_release_execution_roots() {
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let engine = KagariEngine::new(config);
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "operator-failure.kgr",
                r#"use std::ops::{Add, Index};

struct Number {val value:i32}
impl Add<i32> for Number {type Output=i32;fn add(self,rhs:i32)->i32 {self.value/rhs}}
impl Index<i32> for Number {type Output=i32;fn index(self,rhs:i32)->i32 {[self.value][rhs]}}
fn fail_add()->i32 {Number{value:42}+0}
fn fail_index()->i32 {Number{value:42}[2]}
fn exhaust()->i32 {var n=1000;while n>0 {val a=Number{value:n};a+1;n-=1;}42}
fn main()->i32 {Number{value:42}+1}
"#,
            ),
            Default::default(),
        )
        .unwrap();
    let artifact = BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded_program =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let loaded = runtime
        .load_program(&loaded_program, Default::default())
        .unwrap();
    for entry in ["fail_add", "fail_index", "exhaust"] {
        let options = context.clone();
        let cancellation =
            (entry == "exhaust").then(|| support::cancel_after(runtime.runtime(), &loaded, 30));
        assert!(runtime.execute(&loaded, entry, &[], &options).is_err());
        drop(cancellation);
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
        assert!(!runtime.runtime().is_quarantined());
        assert_eq!(
            runtime
                .execute(&loaded, "main", &[], &context)
                .unwrap()
                .return_value
                .value(runtime.runtime().gc())
                .expect("retained execution result"),
            Value::I32(42)
        );
    }
}

#[test]
fn applied_rhs_types_select_overloads_for_syntax_and_methods() {
    execute(
        r#"use std::ops::{Add};

struct Number {val value:i32}
impl Add<i32> for Number {type Output=i32;fn add(self,rhs:i32)->i32 {self.value+rhs}}
impl Add<Number> for Number {type Output=Number;fn add(self,rhs:Number)->Number {Number{value:self.value+rhs.value}}}
fn scalar()->i32 {21}
fn identity<T>(value:T)->T {value}
fn main()->i32 {val a=Number{value:21};val b=a+identity(scalar());if b==42 && a.add(scalar())==42 && a.add(a).value==42 {42}else{0}}
"#,
    );
}

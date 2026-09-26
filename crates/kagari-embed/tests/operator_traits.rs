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
fn ordering_protocols_and_builtin_comparisons_agree() {
    execute(
        r#"
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
        r#"
struct Unknown {}
impl PartialEq for Unknown {fn eq(self,other:Self)->bool {false}}
impl PartialOrd for Unknown {fn partial_cmp(self,other:Self)->Option<Ordering> {None}}
fn main()->i32 {val a=Unknown{}; if !(a<a) && !(a<=a) && !(a>a) && !(a>=a) && a.partial_cmp(a).is_none() {42}else{0}}
"#,
    );
}
#[test]
fn invalid_ordering_contracts_are_diagnostics() {
    for source in [
        "fn main(){val a=Ordering::Equal?;}",
        "fn main()->Ordering {Ordering::Equal?}",
        "fn needs<T:Ord>(v:T){} fn main(){needs(1.0);}",
        "struct X{} impl Ord for X {fn cmp(self,other:Self)->Ordering {Ordering::Equal}} fn main(){}",
        "struct X{} fn main()->bool {X{}<X{}}",
        "fn main()->bool {(1,2)<(2,3)}",
    ] {
        assert!(
            matches!(
                KagariEngine::default().compile_to_artifact(
                    SourceFile::new("bad-order.kgr", source),
                    Default::default(),
                    Default::default()
                ),
                Err(kagari_embed::EmbeddingError::Diagnostics { .. })
            ),
            "{source}"
        );
    }
}

#[test]
fn arithmetic_protocols_have_rhs_and_associated_output() {
    execute(
        r#"
use std::ops::Add as Plus;
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
        r#"
struct Wrap<T>{val item:T}
impl<T:Add<T,Output=T>> Add<Wrap<T>> for Wrap<T> {type Output=Wrap<T>;fn add(self,rhs:Wrap<T>)->Wrap<T> {Wrap{item:self.item+rhs.item}}}
fn main()->i32 {(Wrap{item:20}+Wrap{item:22}).item}
"#,
    );
}

#[test]
fn unary_protocols_support_generic_and_different_output_types() {
    execute(
        r#"
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
        r#"
struct Item {var value:i32}
struct Bag {val items:MutableArray<Item>,var reads:i32}
impl Index<i32> for Bag {type Output=Item;fn index(self,rhs:i32)->Item {self.reads+=1;self.items[rhs]}}
fn read<C:Index<i32>>(c:C,i:i32)->C::Output {c[i]}
fn main()->i32 {
 val item=Item{value:0};val bag=Bag{items:[item],reads:0};
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
            "struct Item {{val value:i32}} struct Bag {{val item:Item}} impl Index<i32> for Bag {{type Output=Item;fn index(self,rhs:i32)->Item {{self.item}}}} fn main() {{val b=Bag{{item:Item{{value:0}}}};{tail}}}"
        );
        assert!(
            matches!(
                KagariEngine::default().compile_to_artifact(
                    SourceFile::new("bad-index.kgr", source),
                    Default::default(),
                    Default::default()
                ),
                Err(kagari_embed::EmbeddingError::Diagnostics { .. })
            ),
            "{tail}"
        );
    }
}

#[test]
fn ordering_aliases_patterns_and_float_unordered_behavior() {
    execute(
        r#"
use std::cmp::Ordering as Order;
use std::cmp::Ordering::*;
fn rank(x:Order)->i32 {match x {Less=>0,Equal=>1,Greater=>2}}
fn main()->i32 {val nan=0.0/0.0; if nan.partial_cmp(nan).is_none() && !(nan<nan) && !(nan<=nan) && !(nan>nan) && !(nan>=nan) && rank(Order::Equal)==1 {42}else{0}}
"#,
    );
}
#[test]
fn operator_operands_and_index_getters_evaluate_once_in_order() {
    execute(
        r#"
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
 std::debug::assert(s.steps==1234 && old.value==42 && b.item.value==99,"getter captures object before RHS");
 s.steps=0;val sum=number(s,1)+number(s,2);
 if s.steps==12 {sum.value}else{0}
}
"#,
    );
}
#[test]
fn invalid_operator_signatures_bounds_and_writes_are_diagnostics() {
    for source in [
        "struct X{} impl Add<X> for X {fn add(self,rhs:X)->X {self}} fn main(){}",
        "struct X{} impl Add<X> for X {type Output=i32;fn add(self,rhs:X)->bool {true}} fn main(){}",
        "struct X{} impl Add<X> for X {type Output=X;fn add(self,rhs:X)->X {self}} fn main(){var a=X{};a+=X{};}",
        "fn f<T:Add<i32,Output=bool>>(x:T){} fn main(){f(1);}",
        "fn f<T:Ord>(x:T){} enum E{A} fn main(){f(E::A);}",
        "fn f<T:Ord>(x:T){} fn main(){f((1,2));}",
        "impl Add<i32> for i32 {type Output=i32;fn add(self,rhs:i32)->i32 {0}} fn main(){}",
        "struct X{} impl Neg for X {type Output=i32;fn neg(self)->i32 {42}} fn main(){val x=X{};val b=x && x;}",
    ] {
        let result = KagariEngine::default().compile_to_artifact(
            SourceFile::new("bad-operator.kgr", source),
            Default::default(),
            Default::default(),
        );
        assert!(
            matches!(
                result,
                Err(kagari_embed::EmbeddingError::Diagnostics { .. })
            ),
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
    use kagari_common::{
        identity::{ModuleIdentity, PackageId},
        source_database::SourceLayer,
    };
    for downstream_override in [false, true] {
        let engine = KagariEngine::default();
        let model = r#"
pub struct Box<T> {pub val value:T}
impl<T:Add<T,Output=T>> Add<T> for Box<T> {type Output=Box<T>;fn add(self,rhs:T)->Box<T> {Box{value:self.value+rhs}}}
impl<T> Index<i32> for Box<T> {type Output=T;fn index(self,rhs:i32)->T {self.value}}
fn plus<T:Add<T,Output=T>>(a:Box<T>,b:T)->Box<T> {a+b}
pub fn add(a:Box<i32>,b:i32)->Box<i32> {plus(a,b)}
"#;
        let root_source = if downstream_override {
            "use pkg::model::Box; impl Neg for Box<i32> {type Output=i32;fn neg(self)->i32 {0}} fn main()->i32 {42}"
        } else {
            "use pkg::model::{Box,add}; fn read<T:Index<i32,Output=i32>>(v:T)->i32 {v[0]} fn main()->i32 {read(add(Box{value:20},1)+21)}"
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
        let checked = engine.compile_snapshot(
            engine.source_snapshot(),
            root.unwrap(),
            Default::default(),
            &Default::default(),
        );
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
        let loaded = runtime.load_program(artifact, Default::default()).unwrap();
        assert_eq!(
            runtime
                .execute(&loaded, "main", &[], &context)
                .unwrap()
                .return_value,
            Value::I32(42)
        );
    }
}

#[test]
fn portable_operator_contracts_reject_wrong_inputs_and_outputs() {
    use kagari_hir::builtin::traits::StandardTrait;
    use kagari_ir::module::{PublicAbiItem, abi::AbiType};
    let artifact = KagariEngine::default()
        .compile_to_artifact(
            SourceFile::new(
                "operator-wire.kgr",
                r#"
struct Number{val value:i32}
impl Add<i32> for Number {type Output=i32;fn add(self,rhs:i32)->i32 {self.value+rhs}}
fn main()->i32 {Number{value:20}+22}
"#,
            ),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    for corrupt_input in [false, true] {
        let mut program = artifact.program.clone();
        let module = &mut program.modules[program.root.index()];
        let table=module.public_items.iter_mut().find_map(|item| match item {
            PublicAbiItem::InterfaceTable(table) if matches!(&table.trait_type,AbiType::Trait(t) if t.declaration==StandardTrait::Add.contract().id)=>Some(table),
            _=>None,
        }).unwrap();
        let AbiType::Trait(interface) = &mut table.trait_type else {
            unreachable!()
        };
        if corrupt_input {
            interface.arguments.clear();
        } else {
            interface.associated_types.clear();
        }
        assert!(kagari_ir::bytecode::verify_program(&program).is_err());
    }
}

#[test]
fn builtin_arithmetic_and_indexing_keep_direct_instructions() {
    use kagari_ir::bytecode::BytecodeInstruction;
    let artifact = KagariEngine::default()
        .compile_to_artifact(
            SourceFile::new(
                "fast-ops.kgr",
                r#"
fn main()->i32 {val a=[40];val b=a[0]+4-2;if b>=42 && !false {-(-b)}else{0}}
"#,
            ),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    assert!(
        !artifact
            .program
            .modules
            .iter()
            .flat_map(|m| &m.functions)
            .flat_map(|f| &f.instructions)
            .any(|i| matches!(i, BytecodeInstruction::Call { .. }))
    );
}

#[test]
fn operator_traps_and_budget_exhaustion_release_execution_roots() {
    let mut config = kagari_embed::EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let engine = KagariEngine::new(config);
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "operator-failure.kgr",
                r#"
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
            Default::default(),
        )
        .unwrap();
    let artifact = BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime.load_program(artifact, Default::default()).unwrap();
    for entry in ["fail_add", "fail_index", "exhaust"] {
        let mut options = context.clone();
        if entry == "exhaust" {
            options.resources.max_instruction_steps = Some(40);
        }
        assert!(runtime.execute(&loaded, entry, &[], &options).is_err());
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
        assert!(!runtime.runtime().is_quarantined());
        assert_eq!(
            runtime
                .execute(&loaded, "main", &[], &context)
                .unwrap()
                .return_value,
            Value::I32(42)
        );
    }
}

#[test]
fn applied_rhs_types_select_overloads_for_syntax_and_methods() {
    execute(
        r#"
struct Number {val value:i32}
impl Add<i32> for Number {type Output=i32;fn add(self,rhs:i32)->i32 {self.value+rhs}}
impl Add<Number> for Number {type Output=Number;fn add(self,rhs:Number)->Number {Number{value:self.value+rhs.value}}}
fn scalar()->i32 {21}
fn identity<T>(value:T)->T {value}
fn main()->i32 {val a=Number{value:21};val b=a+identity(scalar());if b==42 && a.add(scalar())==42 && a.add(a).value==42 {42}else{0}}
"#,
    );
}

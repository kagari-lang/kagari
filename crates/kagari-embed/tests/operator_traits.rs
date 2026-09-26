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
struct Bag {val items:[Item],var reads:i32}
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

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

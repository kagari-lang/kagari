use kagari_common::SourceFile;
use kagari_embed::BytecodeArtifact;
use kagari_embed::ExecutionContext;
use kagari_embed::KagariEngine;
use kagari_embed::program::PreparedProgram;
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
fn set_queries_accept_readonly_and_custom_interfaces() {
    execute(
        r#"
struct Single { val value: f64 }
impl Iterable for Single {
    type Item = f64;
    type Iter = Iter<f64>;
    fn iter(self) -> Iter<f64> { [self.value].iter() }
}
impl Set<f64> for Single {
    fn len(self) -> usize { 1usize }
    fn is_empty(self) -> bool { false }
    fn contains(self, value: f64) -> bool { self.value == value }
}
fn subset<T>(a: Set<T>, b: Set<T>) -> bool { a.is_subset(b) }
fn main() -> i32 {
    val a = LinkedHashSet::from([1, 2]);
    val b: Set<i32> = LinkedHashSet::from([2, 3]);
    val ro: Set<i32> = a;
    std::debug::assert(a.union(b).to_array().get(2usize) == Some(3), "union order");
    std::debug::assert(ro.intersection(b).to_array().get(0usize) == Some(2), "intersection");
    std::debug::assert(ro.difference(b).to_array().get(0usize) == Some(1), "difference");
    std::debug::assert(ro.symmetric_difference(b).to_array().get(1usize) == Some(3), "symmetric order");
    std::debug::assert(!ro.is_subset(b) && !ro.is_superset(b) && !ro.is_disjoint(b), "relations");
    std::debug::assert(subset(ro, ro) && ro.is_superset(ro), "self");
    std::debug::assert(ro.symmetric_difference(ro).is_empty(), "same input");
    val empty: Set<i32> = LinkedHashSet::new();
    std::debug::assert(subset(empty, ro) && ro.is_superset(empty) && empty.is_disjoint(ro), "empty");
    val single: Set<f64> = Single { value: 1.0 };
    val other: Set<f64> = Single { value: 2.0 };
    std::debug::assert(single.is_disjoint(other) && subset(single, single), "no hash bound");
    std::debug::assert(a.len() == 2usize && b.len() == 2usize, "inputs unchanged");
    42
}
"#,
    );
}

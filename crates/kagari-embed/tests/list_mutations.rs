use kagari_common::SourceFile;
use kagari_embed::{BytecodeArtifact, ExecutionContext, KagariEngine, program::PreparedProgram};
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
fn mutations_work_through_shared_interfaces_and_prepare_self_extension() {
    execute(
        r#"
fn extend<T>(target:MutableList<T>, source:List<T>) { target.extend(source); }
fn main() -> i32 {
    val storage=[1,2,3];
    val mutable:MutableList<i32> =storage;
    val view:List<i32> =mutable;
    mutable.swap(0usize,2usize);
    std::debug::assert(view[0usize]==3,"shared swap");
    mutable.reverse();
    std::debug::assert(view[0usize]==1,"shared reverse");
    mutable.truncate(2usize);
    std::debug::assert(view.len()==2usize,"truncate");
    mutable.truncate(9usize);
    extend(mutable,view);
    std::debug::assert(view.len()==4usize,"self extension once");
    std::debug::assert(view[2usize]==1 && view[3usize]==2,"original source");
    std::debug::assert(storage.swap_remove(0usize)==Some(1),"swap removal");
    std::debug::assert(storage[0usize]==2,"last moved");
    std::debug::assert(storage.swap_remove(99usize)==None,"invalid index");
    mutable.extend([]);
    mutable.truncate(0usize);
    std::debug::assert(view.is_empty(),"empty");
    42
}
"#,
    );
}

#[test]
fn capacities_preserve_collection_contents_and_order() {
    execute(
        r#"
fn main() -> i32 {
    val list:ArrayList<i32> = ArrayList::with_capacity(8usize);
    std::debug::assert(list.is_empty() && list.capacity()>=8usize,"list initial");
    list.push(42);
    list.reserve(16usize);
    std::debug::assert(list.capacity()>=17usize && list.len()==1usize && list[0usize]==42,"list reserve");
    val map:LinkedHashMap<i32,String> = LinkedHashMap::with_capacity(8usize);
    map.insert(2,"b");map.insert(1,"a");
    map.reserve(16usize);
    std::debug::assert(map.capacity()>=18usize && map.len()==2usize,"map reserve");
    std::debug::assert(map.keys()[0usize]==2,"map order");
    val set:LinkedHashSet<i32> = LinkedHashSet::with_capacity(8usize);
    set.insert(2);set.insert(1);set.reserve(16usize);
    std::debug::assert(set.capacity()>=18usize && set.len()==2usize,"set reserve");
    std::debug::assert(set.iter().next()==Some(2),"set order");
    for x in list { list.reserve(32usize); }
    std::debug::assert(list[0usize]==42,"reservation does not invalidate iteration");
    42
}
"#,
    );
}

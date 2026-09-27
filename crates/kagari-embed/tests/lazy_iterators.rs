use kagari_common::SourceFile;
use kagari_embed::{BytecodeArtifact, ExecutionContext, KagariEngine};
use kagari_runtime::value::Value;

fn execute(source: &str) {
    let mut config = kagari_embed::EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let engine = KagariEngine::new(config);
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("lazy-iterators.kgr", source),
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
fn terminals_short_circuit_and_preserve_shared_progress() {
    execute(
        r#"
    fn main() -> i32 {
        val values = [1, 20, 22, 99];
        val cursor = values.iter().map(|x| x);
        std::debug::assert_eq(cursor.find(|x| x == 20), Some(20), "find");
        std::debug::assert(cursor.any(|x| x == 22), "any");
        std::debug::assert(!cursor.all(|x| x < 90), "all");
        std::debug::assert_eq(cursor.count(), "".len_bytes(), "end");
        values.push(200);
        val empty = [1].iter().take("".len_bytes());
        std::debug::assert(empty.all(|x| false), "empty all");
        std::debug::assert(!empty.any(|x| true), "empty any");
        val count = [20, 22].iter().count();
        std::debug::assert_eq(count, "ab".len_bytes(), "count");
        var sum = 0;
        [20, 22].iter().for_each(|x| { sum += x; });
        std::debug::assert_eq(sum, 42, "for_each");
        [20, 22].iter().fold(0, |sum, x| sum + x)
    }
    "#,
    );
}

#[test]
fn partition_and_group_by_build_fresh_typed_destinations() {
    execute(
        r#"
    fn main() -> i32 {
        val split: (Array<i32>, Array<i32>) = [20, 1, 22, 3].iter().partition(|x| x % 2 == 0);
        std::debug::assert_eq(split[1][1], 3, "rejected order");
        val groups = [20, 1, 22, 3].iter().group_by(|x| x % 2);
        std::debug::assert_eq(groups.get(1).unwrap_or([0])[1], 3, "group order");
        std::debug::assert_eq(groups.keys()[0], 0, "first key order");
        val group = groups.get(0).unwrap_or([0]);
        group.push(99);
        split[0][0] + split[0][1]
    }
    "#,
    );
}

#[test]
fn grouping_uses_custom_key_protocol_and_partition_uses_user_destination() {
    execute(
        r#"
    struct Key { val value: i32 }
    impl PartialEq for Key { fn eq(self, other: Key) -> bool { self.value == other.value } }
    impl Eq for Key {}
    impl Hash for Key { fn hash(self) -> i64 { 0.hash() } }
    struct Total { val value: i32 }
    impl FromIterator<i32> for Total {
        fn from_iter<I: Iterable<Item = i32>>(source: I) -> Self {
            var value = 0; for item in source { value += item; } Total { value }
        }
    }
    fn main() -> i32 {
        val groups = [20, 1, 22, 3].iter().group_by(|x| Key { value: x % 2 });
        std::debug::assert_eq(groups.len(), "ab".len_bytes(), "collisions");
        std::debug::assert_eq(groups.get(Key { value: 0 }).unwrap_or([0])[1], 22, "custom equality");
        val split: (Total, Total) = [20, 1, 22, 3].iter().partition(|x| x % 2 == 0);
        std::debug::assert_eq(split[1].value, 4, "user destination");
        split[0].value
    }
    "#,
    );
}

#[test]
fn pipelines_reject_invalid_callbacks_keys_and_removed_helpers() {
    for source in [
        "fn main(){[1].iter().filter(|x| x);}",
        "fn main(){[1].iter().group_by(|x| 1.0);}",
        "fn main(){val x:(Array<String>,Array<String>)=[1].iter().partition(|x|true);}",
        "fn main(){[1].iter().filter_map(|x|{val unknown=None;if x==0 {None}else{Some(x)}});}",
        "fn main(){std::iter::len([1]);}",
        "fn main(){std::iter::for_each([1],|x|{});}",
    ] {
        assert!(
            KagariEngine::default()
                .compile_to_artifact(
                    SourceFile::new("invalid-pipeline.kgr", source),
                    Default::default(),
                    Default::default()
                )
                .is_err(),
            "{source}"
        );
    }
}

#[test]
fn adapters_are_lazy_shared_and_collect_without_intermediate_arrays() {
    execute(
        r#"
struct Calls {var count:i32}
fn main()->i32 {
    val calls=Calls{count:0}; val values=[1,20,2,22,100];
    val pipeline=values.iter().filter(|x| x>=20).map(|x| {calls.count+=1;x}).take([0,0].len());
    std::debug::assert_eq(calls.count,0,"construction is lazy");
    val alias=pipeline.iter();
    std::debug::assert_eq(alias.next(),Some(20),"first item");
    std::debug::assert_eq(pipeline.next(),Some(22),"shared progress");
    std::debug::assert_eq(alias.next(),None,"take stops");
    std::debug::assert_eq(calls.count,2,"no extra callback");
    values.push(200);
    val result:Array<i32> = [0,20,0,22].iter().filter_map(|x| if x==0 {None}else{Some(x)}).collect();
    result[0]+result[1]
}
"#,
    );
}

#[test]
fn skip_enumerate_zip_and_chain_preserve_order() {
    execute(
        r#"
fn main()->i32 {
    val pipeline=[99,20,22].iter().skip([0].len()).enumerate().zip(["a","b"]);
    val pairs:Array<((usize,i32),String)> = pipeline.collect();
    std::debug::assert_eq(pairs[0][0][0],"".len_bytes(),"zero index");
    std::debug::assert_eq(pairs[1][0][0],"a".len_bytes(),"next index");
    std::debug::assert_eq(pairs[1][1],"b","right input");
    val chained:Array<i32> = [20].iter().chain([22]).collect();
    chained[0]+chained[1]
}
"#,
    );
}

#[test]
fn custom_iterators_use_native_defaults_and_generic_callbacks() {
    execute(
        r#"
struct Counter {var value:i32}
impl Iterator for Counter {type Item=i32;fn next(self)->Option<i32>{if self.value>22 {None}else{val n=self.value;self.value+=1;Some(n)}}}
fn transform<I:Iterator<Item=i32>>(source:I)->Cursor<i32> {source.filter(|x|x!=21).map(|x|x)}
fn main()->i32 {val result:Array<i32> = transform(Counter{value:20}).collect();result[0]+result[1]}
"#,
    );
}

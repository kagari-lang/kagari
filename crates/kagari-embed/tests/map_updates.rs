use crate::BytecodeArtifact;
use crate::ExecutionContext;
use crate::KagariEngine;
use kagari_common::SourceFile;
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
            let mut backend = kagari_codegen_cranelift::CraneliftBackend::for_host().unwrap();
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
fn map_updates_use_one_callback_and_preserve_shared_values() {
    execute(
        r#"
struct Counter { var count: i32 }
struct Key { val id: i32 }
impl PartialEq for Key { fn eq(self, other: Self) -> bool { self.id == other.id } }
impl Eq for Key {}
impl Hash for Key { fn hash(self) -> i64 { 0i64 } }
fn main() -> i32 {
    val calls = Counter { count: 0 };
    val m = LinkedHashMap::from([(1, 20)]);
    val view: MutableMap<i32, i32> = m;
    val old = view.get_or_insert_with(1, || { calls.count += 1; 99 });
    std::debug::assert(old == 20 && calls.count == 0, "lazy hit");
    std::debug::assert(m.get_or_insert_with(2, || { calls.count += 1; 22 }) == 22, "missing");
    std::debug::assert(view.update(1, |old| { calls.count += 1; old.unwrap_or(0) + 22 }) == 42, "update");
    std::debug::assert(m.update(3, |old| old.unwrap_or(42)) == 42, "update absent");
    std::debug::assert(calls.count == 2 && m.get(1) == Some(42), "once");
    val objects: LinkedHashMap<i32, Counter> = LinkedHashMap::new();
    val shared = objects.get_or_insert_with(1, || calls);
    shared.count = 42;
    std::debug::assert(objects.get(1).unwrap_or(calls).count == 42, "object identity");
    val keyed: MutableMap<Key, i32> = LinkedHashMap::new();
    keyed.get_or_insert_with(Key { id: 1 }, || 40);
    std::debug::assert(keyed.update(Key { id: 1 }, |old| old.unwrap_or(0) + 2) == 42, "custom key");
    std::debug::assert(keyed.len() == 1usize, "equal key");
    42
}
"#,
    );
}

#[test]
fn failed_map_callbacks_preserve_entries_and_release_guards() {
    use kagari_common::{
        collection::CollectionAccess,
        host_interface::{HostFunctionDeclaration, HostInterface, HostValueType},
    };
    use kagari_runtime::host::HostFunction;
    let declaration = HostFunctionDeclaration::new(
        "demo.map",
        vec![],
        HostValueType::Map {
            key: Box::new(HostValueType::I32),
            value: Box::new(HostValueType::I32),
            access: CollectionAccess::Mutable,
        },
    );
    let effects_decl = HostFunctionDeclaration::new(
        "demo.effects",
        vec![],
        HostValueType::Array(Box::new(HostValueType::I32), CollectionAccess::Mutable),
    );
    let profile = kagari_runtime::LanguageProfile {
        allow_host_calls: true,
        ..Default::default()
    };
    let engine = KagariEngine::default();
    engine
        .set_host_interface(HostInterface {
            functions: vec![declaration.clone(), effects_decl.clone()],
            ..Default::default()
        })
        .unwrap();
    for action in [
        "m.insert(1, 99); 42",
        "m.remove(1); 42",
        "m.clear(); 42",
        "m.update(1, |old| 99); 42",
        "m.reserve(8usize); 42",
        "1 / 0",
    ] {
        let source = format!(
            "fn main() {{ val m = demo::map(); val effects = demo::effects(); m.update(1, |old| {{ effects.push(7); {action} }}); }}"
        );
        let artifact = engine
            .compile_to_artifact(
                SourceFile::new("map-callback.kgr", source),
                kagari_embed::CompileOptions {
                    language_profile: profile,
                },
                Default::default(),
            )
            .unwrap();
        let artifact = BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
        let mut context = ExecutionContext {
            language_profile: profile,
            ..Default::default()
        };
        context.capabilities.host_calls = true;
        context.host_policy.allowed_host_functions = vec!["demo.map".into(), "demo.effects".into()];
        let mut runtime = engine.runtime(context.clone());
        let map = runtime
            .runtime()
            .gc()
            .alloc_map(vec![(Value::I32(1), Value::I32(20))])
            .unwrap();
        let map_root = runtime.runtime().gc().root_value(Value::Map(map)).unwrap();
        let effects = runtime.runtime().gc().alloc_array(vec![]).unwrap();
        let effects_root = runtime
            .runtime()
            .gc()
            .root_value(Value::Array(effects))
            .unwrap();
        runtime
            .register_host_function(HostFunction::new(declaration.clone(), move |_, _| {
                Ok(Value::Map(map))
            }))
            .unwrap();
        runtime
            .register_host_function(HostFunction::new(effects_decl.clone(), move |_, _| {
                Ok(Value::Array(effects))
            }))
            .unwrap();
        let program = runtime.load_program(artifact, Default::default()).unwrap();
        let failure = runtime
            .execute(&program, "main", &[], &context)
            .unwrap_err();
        assert_eq!(
            runtime.runtime().gc().map_get(map, &Value::I32(1)),
            Some(Value::I32(20)),
            "{action}: {failure:?}"
        );
        assert_eq!(
            runtime.runtime().gc().array_len(effects),
            Some(1),
            "{action}: {failure:?}"
        );
        runtime
            .runtime()
            .gc()
            .map_insert(map, Value::I32(1), Value::I32(42))
            .unwrap();
        drop((map_root, effects_root));
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
    }
}

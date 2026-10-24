#![cfg(feature = "source")]
use crate::test_support::cancellation as support;
use crate::{BytecodeArtifact, engine::KagariEngine, program::PreparedProgram, runtime::Runtime};
use kagari_bytecode::program::BytecodeProgram;
use kagari_common::identity::metadata::DefinitionMetadata;
use kagari_runtime::{
    Runtime as CoreRuntime, RuntimeConfig, module::Program, native::module::NativeModule,
    value::Value,
};
use kagari_source::source::SourceFile;
use kagari_types::declaration::module::ModuleDecl;
use kagari_vm::vm::Vm;

pub(super) fn prepare_static_program(
    engine: &KagariEngine,
    source: &str,
    decoded: bool,
) -> PreparedProgram {
    let checked = engine
        .compile_source(SourceFile::new("static-values.kgr", source))
        .unwrap();
    if !decoded {
        return engine.prepare(&checked, Default::default()).unwrap();
    }
    let artifact = engine.emit_artifact(&checked, Default::default()).unwrap();
    let bytes = artifact.records().to_bytes(artifact.definitions()).unwrap();
    PreparedProgram::from_artifact(
        BytecodeArtifact::from_bytes(&bytes).unwrap(),
        &Default::default(),
        &Default::default(),
    )
    .unwrap()
}

fn static_call(runtime: &Runtime, module: &Program, entry: &str) -> Value {
    runtime
        .execute(module, entry, &[], &Default::default())
        .unwrap()
        .return_value
        .value(runtime.runtime().gc())
        .unwrap()
}

#[test]
fn persistent_static_char_preserves_state_and_rejects_integer_retyping() {
    let source = r#"
static var STATE: char = 'a';
pub fn read() -> u32 { u32::from(STATE) }
pub fn write() { STATE = '🙂'; }
"#;
    let retyped = r#"
static var STATE: u32 = 97u32;
pub fn read() -> u32 { u32::from(STATE) }
pub fn write() { STATE = 128578u32; }
"#;
    for decoded in [false, true] {
        let engine = KagariEngine::default();
        let first = prepare_static_program(&engine, source, decoded);
        let changed_initializer =
            prepare_static_program(&engine, &source.replace("'a'", "'z'"), decoded);
        let second = prepare_static_program(
            &engine,
            &source.replace("u32::from(STATE)", "u32::from(STATE) + 0u32"),
            decoded,
        );
        let incompatible = prepare_static_program(&engine, retyped, decoded);
        let mut runtime = engine.runtime(Default::default());
        let old = runtime.load_program(&first, Default::default()).unwrap();
        assert_eq!(static_call(&runtime, &old, "read"), Value::I64(97));
        static_call(&runtime, &old, "write");
        assert_eq!(
            runtime
                .reload_program(&old, &changed_initializer, Default::default())
                .unwrap_err()
                .code(),
            "KG_RELOAD_UPDATE_EVIDENCE"
        );
        runtime
            .reload_program(&old, &second, Default::default())
            .unwrap();
        let current = old.clone();
        assert!(
            runtime
                .reload_program(&current, &incompatible, Default::default())
                .is_err()
        );
        for module in [&old, &current] {
            assert_eq!(static_call(&runtime, module, "read"), Value::I64(128578));
        }
    }
}

#[test]
fn persistent_static_objects_and_recursive_nominal_collections_preserve_aliases() {
    let source = r#"
use std::collections::{HashMap, HashSet};
struct Node { var value: i32, val children: Vec<Node> }
enum Envelope { Kept(Option<Result<(String, [Node]), i32>>) }
static var ROOT: Node = Node { value: 1, children: Vec::new() };
static val ALIAS: Node = ROOT;
static val ITEMS: Vec<Option<Node>> = Vec::from([Some(ROOT)]);
static val TABLE: HashMap<String, Vec<Node>> = {
    val map: HashMap<String, Vec<Node>> = HashMap::new();
    map.insert("root", Vec::from([ROOT]));
    map
};
static val KEYS: HashSet<String> = HashSet::from(["root"]);
static val PACKED: Envelope = Envelope::Kept(Some(Ok(("root", [ROOT]))));
static val UNIT: () = ();
pub fn check() -> bool {
    UNIT;
    val item = match ITEMS[0] { Some(node) => node, None => { return false; } };
    val nodes = match TABLE.get("root") { Some(nodes) => nodes, None => { return false; } };
    val packed = match PACKED {
        Envelope::Kept(Some(Ok(pair))) => pair,
        _ => { return false; },
    };
    item === ALIAS && nodes[0] === ALIAS && packed[1][0] === ALIAS
        && KEYS.contains(packed[0])
}
pub fn replace() {
    ROOT = Node { value: 90, children: Vec::new() };
    ALIAS.value = 7;
    ALIAS.children.push(ROOT);
}
pub fn read() -> i32 { ALIAS.value * 100 + ALIAS.children[0].value }
pub fn distinct() -> bool { !(ROOT === ALIAS) }
"#;
    for decoded in [false, true] {
        let engine = KagariEngine::default();
        let first = prepare_static_program(&engine, source, decoded);
        let changed_initializer =
            prepare_static_program(&engine, &source.replace("value: 1,", "value: 2,"), decoded);
        let second = prepare_static_program(
            &engine,
            &source.replace("value: 90,", "value: 91,"),
            decoded,
        );
        let mut runtime = engine.runtime(Default::default());
        let old = runtime.load_program(&first, Default::default()).unwrap();
        assert_eq!(static_call(&runtime, &old, "check"), Value::Bool(true));
        assert_eq!(
            runtime
                .reload_program(&old, &changed_initializer, Default::default())
                .unwrap_err()
                .code(),
            "KG_RELOAD_UPDATE_EVIDENCE"
        );
        runtime
            .reload_program(&old, &second, Default::default())
            .unwrap();
        let current = old.clone();
        static_call(&runtime, &current, "replace");
        for module in [&old, &current] {
            assert_eq!(static_call(&runtime, module, "check"), Value::Bool(true));
            assert_eq!(static_call(&runtime, module, "distinct"), Value::Bool(true));
            assert_eq!(static_call(&runtime, module, "read"), Value::I32(791));
        }
    }
}

const STATIC_PINNING_SOURCE: &str = r#"
use std::collections::{HashMap, HashSet, Map, MutableMap, MutableSet, Set, map_content_equal, set_content_equal};
use std::hash::Hash;
trait Read { fn read(self) -> i32; }
struct Reader {}
impl Read for Reader { fn read(self) -> i32 { version() + STATE } }
trait Keep {
    fn keep<T: Read>(self, value: T) -> fn() -> i32 { || value.read() }
}
impl Keep for i32 {}
fn make_callback() -> fn() -> i32 { val provider: Keep = 0; provider.keep(Reader {}) }
struct Key { val value: i32 }
impl PartialEq for Key { fn eq(self, other: Self) -> bool { self.value % 10 == other.value % 10 } }
impl Eq for Key {}
impl Hash for Key { fn hash(self) -> i64 { 1i64 } }
fn version() -> i32 { 10 }
static var STATE: i32 = 1;
static var CALLBACK: fn() -> i32 = make_callback();
static val FUNCTION: fn() -> i32 = { fn retained() -> i32 { version() } retained };
static val ORIGINAL: fn() -> i32 = CALLBACK;
static var VIEW: Read = Reader {};
static val ORIGINAL_VIEW: Read = VIEW;
static val MAP: HashMap<Key, i32> = {
    val map: HashMap<Key, i32> = HashMap::new();
    map.insert(Key { value: 1 }, 7);
    map
};
static val SET: HashSet<Key> = HashSet::from([Key { value: 1 }]);
pub fn init() -> bool { CALLBACK() == 11 && VIEW.read() == 11 }
pub fn map_lookup() -> bool { MAP.get(Key { value: 11 }) == Some(7) }
pub fn set_lookup() -> bool { SET.contains(Key { value: 11 }) }
pub fn fresh_lookup() -> bool {
    val map: HashMap<Key, i32> = HashMap::new();
    map.insert(Key { value: 1 }, 7);
    map.contains_key(Key { value: 11 })
}
pub fn compare_contents() -> i32 {
    val other_map: HashMap<Key, i32> = HashMap::new();
    other_map.insert(Key { value: 11 }, 7);
    val other_set = HashSet::from([Key { value: 11 }]);
    var matches = 0;
    if MAP == other_map { matches += 1; }
    if map_content_equal(MAP, other_map) { matches += 2; }
    if SET == other_set { matches += 4; }
    if set_content_equal(SET, other_set) { matches += 8; }
    matches
}
pub fn content_hashes() -> bool {
    val other_map: HashMap<Key, i32> = HashMap::new();
    other_map.insert(Key { value: 1 }, 7);
    val other_set = HashSet::from([Key { value: 1 }]);
    MAP.hash() == other_map.hash() && SET.hash() == other_set.hash()
}
pub fn mutate_keys() -> bool {
    MAP.insert(Key { value: 21 }, 9);
    SET.insert(Key { value: 21 });
    val map: MutableMap<Key, i32> = MAP;
    val set: MutableSet<Key> = SET;
    if map.len() != 1u64 || set.len() != 1u64 { return false; }
    map.insert(Key { value: 31 }, 12);
    set.insert(Key { value: 31 });
    val read_map: Map<Key, i32> = map;
    val read_set: Set<Key> = set;
    val lookup = Key { value: 11 };
    if read_map.get(lookup) != Some(12) || !read_set.contains(lookup) {
        return false;
    }
    val removed = Key { value: 41 };
    if map.remove(removed) != Some(12) || !set.remove(removed) {
        return false;
    }
    if !map.is_empty() || !set.is_empty() { return false; }
    MAP.insert(Key { value: 2 }, 15);
    SET.insert(Key { value: 2 });
    map.clear();
    set.clear();
    map.insert(Key { value: 3 }, 17);
    set.insert(Key { value: 3 });
    MAP.contains_key(Key { value: 13 }) && MAP.get(Key { value: 13 }) == Some(17)
        && SET.contains(Key { value: 13 })
}
pub fn function() -> i32 { FUNCTION() }
pub fn set_state() { STATE = 5; }
pub fn saved() -> i32 { ORIGINAL() + ORIGINAL_VIEW.read() }
pub fn current() -> i32 { CALLBACK() + VIEW.read() }
pub fn replace() { CALLBACK = make_callback(); VIEW = Reader {}; }
"#;

#[test]
fn persistent_static_callbacks_and_interfaces_keep_generations() {
    let source = STATIC_PINNING_SOURCE;
    // Existing initializer dependencies stay frozen. Explicit assignment may
    // install new values made by newly added, unprotected code.
    let replacement = format!(
        "{}\nstruct UpdatedReader {{}}\nimpl Read for UpdatedReader {{ fn read(self) -> i32 {{ 20 + STATE }} }}",
        source.replace(
            "CALLBACK = make_callback(); VIEW = Reader {};",
            "CALLBACK = || 20 + STATE; VIEW = UpdatedReader {};",
        )
    );
    for decoded in [false, true] {
        let engine = KagariEngine::default();
        let first = prepare_static_program(&engine, source, decoded);
        let second = prepare_static_program(&engine, &replacement, decoded);
        let mut runtime = engine.runtime(Default::default());
        let old = runtime.load_program(&first, Default::default()).unwrap();
        assert_eq!(static_call(&runtime, &old, "init"), Value::Bool(true));
        // Force the aliases before replacing their original cells.
        assert_eq!(static_call(&runtime, &old, "saved"), Value::I32(22));
        assert_eq!(static_call(&runtime, &old, "function"), Value::I32(10));
        runtime
            .reload_program(&old, &second, Default::default())
            .unwrap();
        let current = old.clone();
        static_call(&runtime, &current, "set_state");

        assert_eq!(
            static_call(&runtime, &current, "fresh_lookup"),
            Value::Bool(true)
        );
        assert_eq!(static_call(&runtime, &current, "current"), Value::I32(30));
        assert_eq!(static_call(&runtime, &current, "function"), Value::I32(10));
        static_call(&runtime, &current, "replace");
        assert_eq!(static_call(&runtime, &current, "current"), Value::I32(50));
        assert_eq!(static_call(&runtime, &current, "saved"), Value::I32(30));
        assert_eq!(static_call(&runtime, &old, "current"), Value::I32(50));
    }
}

#[test]
fn persistent_static_hash_comparators_reject_changes_and_preserve_values() {
    let source = STATIC_PINNING_SOURCE;
    let replacement = source
        .replace(
            "self.value % 10 == other.value % 10",
            "self.value == other.value",
        )
        .replace("{ 1i64 }", "{ 2i64 }");
    let mut results = Vec::new();
    for decoded in [false, true] {
        let engine = KagariEngine::default();
        let first = prepare_static_program(&engine, source, decoded);
        let second = prepare_static_program(&engine, &replacement, decoded);
        let mut runtime = engine.runtime(Default::default());
        let old = runtime.load_program(&first, Default::default()).unwrap();
        for entry in ["map_lookup", "set_lookup"] {
            assert_eq!(static_call(&runtime, &old, entry), Value::Bool(true));
        }
        assert_eq!(
            static_call(&runtime, &old, "compare_contents"),
            Value::I32(15)
        );
        assert_eq!(
            runtime
                .reload_program(&old, &second, Default::default())
                .unwrap_err()
                .code(),
            "KG_RELOAD_UPDATE_EVIDENCE"
        );
        runtime
            .reload_program(&old, &first, Default::default())
            .unwrap();
        let current = old.clone();
        assert_eq!(
            static_call(&runtime, &current, "compare_contents"),
            Value::I32(15)
        );
        assert_eq!(
            static_call(&runtime, &current, "content_hashes"),
            Value::Bool(true)
        );
        assert_eq!(
            static_call(&runtime, &current, "fresh_lookup"),
            Value::Bool(true)
        );
        for entry in ["map_lookup", "set_lookup", "mutate_keys"] {
            results.push((decoded, entry, static_call(&runtime, &current, entry)));
        }
    }
    assert!(
        results
            .iter()
            .all(|(_, _, result)| *result == Value::Bool(true)),
        "retained collection lookups: {results:?}"
    );
}

fn prepare(engine: &KagariEngine, source: &str) -> DefinitionMetadata<BytecodeProgram> {
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("generic-reload.kgr", source),
            Default::default(),
        )
        .unwrap();
    let (definitions, artifact) =
        BytecodeArtifact::from_bytes(&artifact.records().to_bytes(artifact.definitions()).unwrap())
            .unwrap()
            .into_parts();
    DefinitionMetadata::checked(definitions, artifact.program, &Default::default()).unwrap()
}

#[test]
fn retained_local_function_values_pin_generations_and_clean_up_all_exits() {
    let source = r#"
trait Run { fn run(self, mode: i32) -> i32; }
struct Saved { val callback: fn(i32) -> i32 }
impl Run for Saved { fn run(self, mode: i32) -> i32 { (self.callback)(mode) } }
trait Keep {
    fn keep<F: Fn(i32) -> i32>(self, callback: F) -> Run {
        Saved { callback: |mode| {
            val boxed: Fn<(i32,), Output=i32> = callback;
            boxed.call((mode,))
        } }
    }
}
impl Keep for i32 {}
pub fn make() -> Run {
    fn local(mode: i32) -> i32 {
        val items = Vec::from([mode]);
        fn work(items: Vec<i32>) -> i32 {
            if items[0] == 0 { 42 }
            else if items[0] == 1 { val zero = items[0] - 1; 1 / zero }
            else if items[0] == 2 { work(items) }
            else { loop {} }
        }
        work(items)
    }
    val keeper: Keep = 0;
    keeper.keep(local)
}
"#;
    let engine = KagariEngine::default();
    let program = prepare(&engine, source);
    let replacement = prepare(&engine, &source.replace("{ 42 }", "{ 43 }"));
    let mut config = RuntimeConfig::default();
    config.gc.collection_threshold = Some(1);
    config.limits.max_call_depth = Some(32);
    let mut runtime = CoreRuntime::new(config);
    NativeModule::install_all(&kagari_stdlib::modules().unwrap(), &mut runtime).unwrap();
    let old = runtime.load_program("local-reload", program).unwrap();
    let _old_program = old;
    let old = runtime.select_program(&_old_program).unwrap();
    let contract = old
        .bytecode
        .trait_contracts
        .iter()
        .find(|contract| contract.abi.name == "Run")
        .unwrap();
    let run = ModuleDecl::method_id(contract.declaration, "run", old.definitions()).unwrap();
    let old_key = old.key();
    let vm = Vm::new(runtime);
    let saved_root = vm.execute(&old, "make").unwrap().return_value;
    let saved = saved_root.value(vm.runtime().gc()).unwrap();
    vm.reload_program(&_old_program, Some("local-reload".into()), replacement)
        .unwrap();
    let _current_program = _old_program.clone();
    let current = vm.runtime().select_program(&_current_program).unwrap();
    drop(old);
    vm.runtime().collect_garbage().unwrap();
    assert!(vm.runtime().modules().loaded(old_key).is_some());
    let fresh_root = vm.execute(&current, "make").unwrap().return_value;
    let fresh = fresh_root.value(vm.runtime().gc()).unwrap();
    for (value, expected) in [(&saved, 42), (&fresh, 43)] {
        assert_eq!(
            vm.invoke_interface_method(value, &run, &[Value::I32(0)])
                .unwrap(),
            Value::I32(expected)
        );
    }
    for (mode, expected) in [(1, "division by zero"), (2, "call depth")] {
        let error = vm
            .invoke_interface_method(&saved, &run, &[Value::I32(mode)])
            .unwrap_err();
        assert!(format!("{error:?}").contains(expected), "{error:?}");
        assert_eq!(vm.runtime().gc().active_roots(), 2);
        assert!(!vm.runtime().is_quarantined());
    }
    let session = support::cancel_after(vm.runtime(), &current, 48);
    let error = vm
        .invoke_interface_method(&fresh, &run, &[Value::I32(3)])
        .unwrap_err();
    drop(session);
    assert!(format!("{error:?}").contains("cancel"), "{error:?}");
    assert_eq!(vm.runtime().gc().active_roots(), 2);
    assert!(!vm.runtime().is_quarantined());
    assert_eq!(
        vm.invoke_interface_method(&saved, &run, &[Value::I32(0)])
            .unwrap(),
        Value::I32(42)
    );
    drop((saved_root, fresh_root));
    assert_eq!(vm.runtime().gc().active_roots(), 0);
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
    assert_eq!(
        vm.runtime()
            .modules()
            .retention_counts(old_key)
            .runtime_values,
        0
    );
}

#[test]
fn retained_generic_default_and_override_closures_pin_types_and_operations_after_reload() {
    let method = "fn keep<T: Ord + Default + Read>(self, a: T, b: T) -> fn(i32) -> bool { val early: Read = a; |mode| if mode == 1 { T::default() == a } else if mode == 2 { early.read() } else if mode == 3 { val late: Read = a; late.read() } else { a.cmp(b) == Ordering::Less } }";
    for implementation in [String::new(), method.to_owned()] {
        let source = format!(
            r#"use std::cmp::{{Ordering}};

trait Read {{ fn read(self) -> bool; }}
trait Keep {{ {method} }}
impl Keep for i32 {{ {implementation} }}
struct Rank {{ val value: i32 }}
impl Read for Rank {{ fn read(self) -> bool {{ true }} }}
impl Default for Rank {{ fn default() -> Self {{ Rank {{ value: 1 }} }} }}
impl PartialEq for Rank {{ fn eq(self, other: Self) -> bool {{ self.value == other.value }} }}
impl Eq for Rank {{}}
impl PartialOrd for Rank {{ fn partial_cmp(self, other: Self) -> Option<Ordering> {{ self.value.partial_cmp(other.value) }} }}
impl Ord for Rank {{ fn cmp(self, other: Self) -> Ordering {{ self.value.cmp(other.value) }} }}
trait Run {{ fn run(self) -> bool; fn selected_default(self) -> bool; fn constructed(self) -> bool; fn late_constructed(self) -> bool; }}
struct Saved {{ val callback: fn(i32) -> bool }}
impl Run for Saved {{
    fn run(self) -> bool {{ (self.callback)(0) }}
    fn selected_default(self) -> bool {{ (self.callback)(1) }}
    fn constructed(self) -> bool {{ (self.callback)(2) }}
    fn late_constructed(self) -> bool {{ (self.callback)(3) }}
}}
pub fn make() -> Run {{
    val keeper: Keep = 0;
    Saved {{ callback: keeper.keep(Rank {{ value: 1 }}, Rank {{ value: 2 }}) }}
}}
"#
        );
        let engine = KagariEngine::default();
        let program = prepare(&engine, &source);
        let replacement = prepare(
            &engine,
            &source
                .replace("self.value.cmp(other.value)", "other.value.cmp(self.value)")
                .replace(
                    "fn read(self) -> bool { true }",
                    "fn read(self) -> bool { false }",
                )
                .replace(
                    "fn default() -> Self { Rank { value: 1 } }",
                    "fn default() -> Self { Rank { value: 2 } }",
                ),
        );
        let mut config = RuntimeConfig::default();
        config.gc.collection_threshold = Some(1);
        let mut runtime = {
            let mut registered = CoreRuntime::new(config);
            kagari_runtime::native::module::NativeModule::install_all(
                &kagari_stdlib::modules().unwrap(),
                &mut registered,
            )
            .unwrap();
            registered
        };
        let old = runtime.load_program("generic-reload", program).unwrap();
        let _old_program = old;
        let old = runtime.select_program(&_old_program).unwrap();
        let contract = old
            .bytecode
            .trait_contracts
            .iter()
            .find(|contract| contract.abi.name == "Run")
            .unwrap();
        let run = ModuleDecl::method_id(contract.declaration, "run", old.definitions()).unwrap();
        let selected_default =
            ModuleDecl::method_id(contract.declaration, "selected_default", old.definitions())
                .unwrap();
        let constructed =
            ModuleDecl::method_id(contract.declaration, "constructed", old.definitions()).unwrap();
        let late_constructed =
            ModuleDecl::method_id(contract.declaration, "late_constructed", old.definitions())
                .unwrap();
        let old_key = old.key();
        let vm = Vm::new(runtime);
        let saved = vm
            .execute(&old, "make")
            .unwrap()
            .return_value
            .value(vm.runtime().gc())
            .expect("retained execution result");
        let root = vm.runtime().root_value(saved).unwrap();
        vm.reload_program(&_old_program, Some("generic-reload".into()), replacement)
            .unwrap();
        let _current_program = _old_program.clone();
        let current = vm.runtime().select_program(&_current_program).unwrap();
        drop(old);
        vm.runtime().collect_garbage().unwrap();
        assert!(vm.runtime().modules().loaded(old_key).is_some());
        let fresh = vm
            .execute(&current, "make")
            .unwrap()
            .return_value
            .value(vm.runtime().gc())
            .expect("retained execution result");
        let fresh_root = vm.runtime().root_value(fresh).unwrap();
        assert_eq!(
            vm.invoke_interface_method(&saved, &run, &[]).unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            vm.invoke_interface_method(&fresh, &run, &[]).unwrap(),
            Value::Bool(false)
        );
        assert_eq!(
            vm.invoke_interface_method(&saved, &selected_default, &[])
                .unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            vm.invoke_interface_method(&fresh, &selected_default, &[])
                .unwrap(),
            Value::Bool(false)
        );
        assert_eq!(
            vm.invoke_interface_method(&saved, &constructed, &[])
                .unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            vm.invoke_interface_method(&fresh, &constructed, &[])
                .unwrap(),
            Value::Bool(false)
        );
        assert_eq!(
            vm.invoke_interface_method(&saved, &late_constructed, &[])
                .unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            vm.invoke_interface_method(&fresh, &late_constructed, &[])
                .unwrap(),
            Value::Bool(false)
        );
        drop((root, fresh_root));
        assert_eq!(vm.runtime().gc().active_roots(), 0);
        assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
        assert_eq!(
            vm.runtime()
                .modules()
                .retention_counts(old_key)
                .runtime_values,
            0
        );
    }
}

#[test]
fn retained_generic_try_calls_keep_the_selected_carrier_generation() {
    let source = r#"
use core::ops::{Try, FromResidual, ControlFlow};
use core::convert::Infallible;
enum Carrier { Value(i32), Stopped }
impl FromResidual<Option<Infallible>> for Carrier {
    fn from_residual(value:Option<Infallible>)->Self {Carrier::Stopped}
}
impl Try for Carrier {
    type Output = i32;
    type Residual = Option<Infallible>;
    fn from_output(value:i32)->Self {Carrier::Value(value)}
    fn branch(self)->ControlFlow<Option<Infallible>,i32> {
        match self { Carrier::Value(value) => ControlFlow::Continue(value), Carrier::Stopped => ControlFlow::Break(None) }
    }
}
trait Keep {
    fn keep<A: Try<Output = i32>, R: FromResidual<A::Residual>>(self, value:A, wrap:fn(i32)->R)->fn()->R { || wrap(value?) }
}
impl Keep for i32 {}
trait Run { fn run(self)->i32; }
struct Saved { val callback:fn()->Option<i32> }
impl Run for Saved { fn run(self)->i32 { match (self.callback)() {Some(value)=>value,None=>0} } }
pub fn make()->Run {val keeper:Keep=0; Saved {callback:keeper.keep(Carrier::Value(20),|value| Some(value+22))}}
pub fn make_stopped()->Run {val keeper:Keep=0; Saved {callback:keeper.keep(Carrier::Stopped,|value| Some(value+22))}}
"#;
    let engine = KagariEngine::default();
    let program = prepare(&engine, source);
    let replacement = prepare(
        &engine,
        &source.replace(
            "ControlFlow::Continue(value)",
            "ControlFlow::Continue(value + 1)",
        ),
    );
    let mut config = RuntimeConfig::default();
    config.gc.collection_threshold = Some(1);
    let mut runtime = CoreRuntime::new(config);
    kagari_runtime::native::module::NativeModule::install_all(
        &kagari_stdlib::modules().unwrap(),
        &mut runtime,
    )
    .unwrap();
    let old = runtime.load_program("try-reload", program).unwrap();
    let _old_program = old;
    let old = runtime.select_program(&_old_program).unwrap();
    let contract = old
        .bytecode
        .trait_contracts
        .iter()
        .find(|contract| contract.abi.name == "Run")
        .unwrap();
    let run = ModuleDecl::method_id(contract.declaration, "run", old.definitions()).unwrap();
    let old_key = old.key();
    let vm = Vm::new(runtime);
    let saved = vm
        .execute(&old, "make")
        .unwrap()
        .return_value
        .value(vm.runtime().gc())
        .expect("retained execution result");
    let root = vm.runtime().root_value(saved).unwrap();
    let stopped = vm
        .execute(&old, "make_stopped")
        .unwrap()
        .return_value
        .value(vm.runtime().gc())
        .expect("retained execution result");
    let stopped_root = vm.runtime().root_value(stopped).unwrap();
    vm.reload_program(&_old_program, Some("try-reload".into()), replacement)
        .unwrap();
    let _current_program = _old_program.clone();
    let current = vm.runtime().select_program(&_current_program).unwrap();
    drop(old);
    vm.runtime().collect_garbage().unwrap();
    assert!(vm.runtime().modules().loaded(old_key).is_some());
    let fresh = vm
        .execute(&current, "make")
        .unwrap()
        .return_value
        .value(vm.runtime().gc())
        .expect("retained execution result");
    let fresh_root = vm.runtime().root_value(fresh).unwrap();
    let fresh_stopped = vm
        .execute(&current, "make_stopped")
        .unwrap()
        .return_value
        .value(vm.runtime().gc())
        .expect("retained execution result");
    let fresh_stopped_root = vm.runtime().root_value(fresh_stopped).unwrap();
    assert_eq!(
        vm.invoke_interface_method(&saved, &run, &[]).unwrap(),
        Value::I32(42)
    );
    assert_eq!(
        vm.invoke_interface_method(&fresh, &run, &[]).unwrap(),
        Value::I32(43)
    );
    for value in [&stopped, &fresh_stopped] {
        assert_eq!(
            vm.invoke_interface_method(value, &run, &[]).unwrap(),
            Value::I32(0)
        );
    }
    drop((root, fresh_root, stopped_root, fresh_stopped_root));
    assert_eq!(vm.runtime().gc().active_roots(), 0);
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
    assert_eq!(
        vm.runtime()
            .modules()
            .retention_counts(old_key)
            .runtime_values,
        0
    );
}

#[test]
fn content_helper_retains_interface_iteration_generation_in_a_new_caller() {
    let source = r#"
use std::collections::{List, list_content_equal};
use std::iter::CollectionCursor;
use std::ops::Index;
struct Bag { val value: f32 }
impl Index<u64> for Bag { type Output = f32; fn index(self, index: u64) -> f32 { self.value } }
impl Iterable for Bag {
    type Item = f32;
    type Iter = CollectionCursor<f32>;
    fn iter(self) -> CollectionCursor<f32> { Vec::from([self.value]).iter() }
}
impl List<f32> for Bag {
    fn len(self) -> u64 { 1u64 }
    fn is_empty(self) -> bool { false }
    fn get(self, index: u64) -> Option<f32> { Some(self.value) }
}
trait Probe { fn same(self, a: List<f32>, b: List<f32>) -> bool; }
struct Checker {}
impl Probe for Checker { fn same(self, a: List<f32>, b: List<f32>) -> bool { list_content_equal(a, b) } }
pub fn make() -> List<f32> { Bag { value: 42.0f32 } }
pub fn probe() -> Probe { Checker {} }
"#;
    let engine = KagariEngine::default();
    let original = prepare(&engine, source);
    let replacement = prepare(
        &engine,
        &source.replace("Vec::from([self.value])", "Vec::from([0.0f32 / 0.0f32])"),
    );
    let mut config = RuntimeConfig::default();
    config.gc.collection_threshold = Some(1);
    let mut runtime = CoreRuntime::new(config);
    NativeModule::install_all(&kagari_stdlib::modules().unwrap(), &mut runtime).unwrap();
    let old = runtime.load_program("receiver-reload", original).unwrap();
    let _old_program = old;
    let old = runtime.select_program(&_old_program).unwrap();
    let old_key = old.key();
    let vm = Vm::new(runtime);
    let saved = vm.execute(&old, "make").unwrap().return_value;
    vm.reload_program(&_old_program, Some("receiver-reload".into()), replacement)
        .unwrap();
    let _current_program = _old_program.clone();
    let current = vm.runtime().select_program(&_current_program).unwrap();
    drop(old);
    vm.runtime().collect_garbage().unwrap();
    assert!(vm.runtime().modules().loaded(old_key).is_some());
    let fresh = vm.execute(&current, "make").unwrap().return_value;
    let probe = vm.execute(&current, "probe").unwrap().return_value;
    let contract = current
        .bytecode
        .trait_contracts
        .iter()
        .find(|contract| contract.abi.name == "Probe")
        .unwrap();
    let method =
        ModuleDecl::method_id(contract.declaration, "same", current.definitions()).unwrap();
    for (root, expected) in [(&saved, true), (&fresh, false)] {
        let value = root.value(vm.runtime().gc()).unwrap();
        assert_eq!(
            vm.invoke_interface_method(
                &probe.value(vm.runtime().gc()).unwrap(),
                &method,
                &[value, value]
            )
            .unwrap(),
            Value::Bool(expected)
        );
    }
    drop((saved, fresh, probe));
    assert_eq!(vm.runtime().gc().active_roots(), 0);
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
    assert_eq!(
        vm.runtime()
            .modules()
            .retention_counts(old_key)
            .runtime_values,
        0
    );
}

#[test]
fn shared_collection_comparison_retains_the_callers_nominal_generation() {
    assert_shared_collection_comparison_reloading(SHARED_COLLECTION_SOURCE, false, |source| {
        source.replace("bool { true }", "bool { false }")
    });
}

#[test]
fn shared_collection_comparison_accepts_new_primitive_applications_after_reload() {
    assert_shared_collection_comparison_reloading(SHARED_COLLECTION_SOURCE, false, |source| {
        source
            .replace("Item { value: 1 }", "0.0f32 / 0.0f32")
            .replace("Item { value: 2 }", "0.0f32 / 0.0f32")
    });
}

#[test]
fn content_helper_selects_element_operations_in_the_callers_generation() {
    let source = mixed_generation_collection_source();
    // Element comparison belongs to the helper caller; iteration remains pinned to the view.
    assert_shared_collection_comparison_reloading(&source, false, |source| {
        source.replace(
            "impl PartialEq for Local { fn eq(self, other: Self) -> bool { true } }",
            "impl PartialEq for Local { fn eq(self, other: Self) -> bool { false } }",
        )
    });
}

#[test]
fn shared_collection_comparison_composes_the_new_argument_operation() {
    let source = mixed_generation_collection_source();
    assert_shared_collection_comparison_reloading(&source, false, |source| {
        source.replace("bool { true }", "bool { false }")
    });
    let same_nominal = source
        .replace("Item { value: 1 }", "Local {}")
        .replace("Item { value: 2 }", "Local {}");
    assert_shared_collection_comparison_reloading(&same_nominal, false, |source| {
        source.replace("bool { true }", "bool { false }")
    });
}

#[test]
fn shared_tuple_adapter_preserves_each_arguments_generation() {
    let source = r#"
use std::collections::list_content_equal;
fn same<T: PartialEq>(left: T, right: T) -> bool { left == right }
struct Local {}
impl PartialEq for Local { fn eq(self, other: Self) -> bool { true } }
struct Item { val value: i32 }
impl PartialEq for Item { fn eq(self, other: Self) -> bool { true } }
trait Boxer {
    fn one<T: PartialEq>(self, left: T, right: T) -> bool {
        val a = (Local {}, left);
        val b = (Local {}, right);
        val direct = a == b;
        val forwarded = same(a, b);
        val native = list_content_equal(Vec::from([a]), Vec::from([b]));
        val collection = same(Vec::from([a]), Vec::from([b]));
        if direct != forwarded || direct != native || direct != collection { val zero = 0; 1 / zero; }
        direct
    }
}
struct Factory {}
impl Boxer for Factory {}
trait Probe { fn check(self, factory: Boxer) -> bool; }
struct Checker {}
impl Probe for Checker {
    fn check(self, factory: Boxer) -> bool {
        factory.one(Item { value: 1 }, Item { value: 2 })
    }
}
pub fn make() -> Boxer { Factory {} }
pub fn probe() -> Probe { Checker {} }
"#;
    assert_shared_collection_comparison_reloading(source, true, |source| {
        source.replace(
            "impl PartialEq for Local { fn eq(self, other: Self) -> bool { true } }",
            "impl PartialEq for Local { fn eq(self, other: Self) -> bool { false } }",
        )
    });
    assert_shared_collection_comparison_reloading(source, false, |source| {
        source.replace("bool { true }", "bool { false }")
    });
    let same_nominal = source
        .replace("Item { value: 1 }", "Local {}")
        .replace("Item { value: 2 }", "Local {}");
    assert_shared_collection_comparison_reloading(&same_nominal, false, |source| {
        source.replace("bool { true }", "bool { false }")
    });
}

fn mixed_generation_collection_source() -> String {
    SHARED_COLLECTION_SOURCE
        .replace("struct Item", "struct Local {}\nimpl PartialEq for Local { fn eq(self, other: Self) -> bool { true } }\nstruct Item")
        .replace("List<T>", "List<(Local, T)>")
        .replace("Vec::from([value])", "Vec::from([(Local {}, value)])")
}

const SHARED_COLLECTION_SOURCE: &str = r#"
use std::collections::{List, list_content_equal};
struct Item { val value: i32 }
impl PartialEq for Item { fn eq(self, other: Self) -> bool { true } }
fn pack<T>(value: T) -> List<T> { Vec::from([value]) }
trait Boxer {
    fn one<T>(self, value: T) -> List<T> {
        val make = || pack(value);
        make()
    }
}
struct Factory {}
impl Boxer for Factory {}
trait Probe { fn check(self, factory: Boxer) -> bool; }
struct Checker {}
impl Probe for Checker {
    fn check(self, factory: Boxer) -> bool {
        list_content_equal(factory.one(Item { value: 1 }), factory.one(Item { value: 2 }))
    }
}
pub fn make() -> Boxer { Factory {} }
pub fn probe() -> Probe { Checker {} }
"#;

fn assert_shared_collection_comparison_reloading(
    source: &str,
    new_expected: bool,
    replacement: impl FnOnce(&str) -> String,
) {
    let engine = KagariEngine::default();
    let original = prepare(&engine, source);
    let replacement = prepare(&engine, &replacement(source));
    let mut config = RuntimeConfig::default();
    config.gc.collection_threshold = Some(1);
    let mut runtime = CoreRuntime::new(config);
    NativeModule::install_all(&kagari_stdlib::modules().unwrap(), &mut runtime).unwrap();
    let old = runtime
        .load_program("shared-comparison-reload", original)
        .unwrap();
    let _old_program = old;
    let old = runtime.select_program(&_old_program).unwrap();
    let old_key = old.key();
    let vm = Vm::new(runtime);
    let factory = vm.execute(&old, "make").unwrap().return_value;
    let old_probe = vm.execute(&old, "probe").unwrap().return_value;
    vm.reload_program(
        &_old_program,
        Some("shared-comparison-reload".into()),
        replacement,
    )
    .unwrap();
    let _current_program = _old_program.clone();
    let current = vm.runtime().select_program(&_current_program).unwrap();
    drop(old);
    vm.runtime().collect_garbage().unwrap();
    let fresh_probe = vm.execute(&current, "probe").unwrap().return_value;
    let contract = current
        .bytecode
        .trait_contracts
        .iter()
        .find(|contract| contract.abi.name == "Probe")
        .unwrap();
    let method =
        ModuleDecl::method_id(contract.declaration, "check", current.definitions()).unwrap();
    for (probe, expected) in [(&old_probe, true), (&fresh_probe, new_expected)] {
        assert_eq!(
            vm.invoke_interface_method(
                &probe.value(vm.runtime().gc()).unwrap(),
                &method,
                &[factory.value(vm.runtime().gc()).unwrap()],
            )
            .unwrap(),
            Value::Bool(expected),
        );
    }
    drop((factory, old_probe, fresh_probe));
    assert_eq!(vm.runtime().gc().active_roots(), 0);
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
    assert_eq!(
        vm.runtime()
            .modules()
            .retention_counts(old_key)
            .runtime_values,
        0,
    );
}

#[test]
fn bound_construction_uses_the_callers_generation_with_an_old_shared_provider() {
    let source = r#"
trait Read { fn read(self) -> bool; }
struct Item { val value: i32 }
impl Read for Item { fn read(self) -> bool { true } }
trait Source { type Item: Read; }
struct Tag {}
impl Source for Tag { type Item = Item; }
fn erase<T: Read>(value: T) -> Read { value }
trait Boxer {
    fn one<S: Source>(self, source: S, value: S::Item) -> Read { erase(value) }
}
struct Factory {}
impl Boxer for Factory {}
trait Probe { fn check(self, factory: Boxer) -> bool; }
struct Checker {}
impl Probe for Checker {
    fn check(self, factory: Boxer) -> bool { factory.one(Tag {}, Item { value: 42 }).read() }
}
pub fn make() -> Boxer { Factory {} }
pub fn probe() -> Probe { Checker {} }
"#;
    assert_shared_collection_comparison_reloading(source, false, |source| {
        source.replace("bool { true }", "bool { false }")
    });
}

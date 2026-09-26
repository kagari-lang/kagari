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
fn constructors_views_and_shallow_factories() {
    execute(
        r#"
fn main() -> i32 {
    val a = MutableArray::from([1, 2]);
    val r: [i32] = a;
    a.push(3);
    std::debug::assert(r.len() == [0, 0, 0].len(), "live view");
    val snapshot = Array::from(a);
    a.push(4);
    std::debug::assert(snapshot.len() == [0, 0, 0].len(), "snapshot");
    val m = MutableMap::from([("a", 1), ("a", 2)]);
    val mr: Map<String, i32> = m;
    std::debug::assert(mr.get("a") == Some(2), "duplicate");
    m.insert("b", 3);
    std::debug::assert(mr.contains_key("b"), "map view");
    val s = Set::from([1, 1, 2]);
    std::debug::assert(s.len() == [0, 0].len(), "dedup");
    val ms = MutableSet::from([1]);
    ms.insert(2);
    val empty: Map<String, i32> = Map::new();
    val empty_array: MutableArray<i32> = MutableArray::new();
    val empty_set: Set<i32> = Set::from([]);
    std::debug::assert(empty.is_empty() && empty_array.is_empty() && empty_set.is_empty(), "empty");
    42
}
"#,
    );
}

#[test]
fn access_is_shallow_and_preserved_by_calls_closures_and_branch_joins() {
    execute(
        r#"
struct Item { var value: i32 }
struct Shelf { val items: [Item] }
fn readable<T>(values: MutableArray<T>) -> [T] { values }
fn size<T>(values: [T]) -> usize { values.len() }
fn main() -> i32 {
    val item = Item { value: 1 };
    val writable = [item];
    val view = readable(writable);
    val shelf = Shelf { items: writable };
    shelf.items[0].value = 40;
    val copy = Array::from(view);
    writable[0] = Item { value: 7 };
    std::debug::assert(copy[0] === item && copy !== view, "shallow independent slots");
    std::debug::assert(view === writable && view == writable, "common access equality");
    std::debug::assert(view.hash() == writable.hash(), "common identity hash");
    val join = if true { view } else { writable };
    val other = match true { true => writable, false => view };
    val inspect = || size(join) == size(other);
    std::debug::assert(inspect(), "captured view");
    val nested: [MutableArray<i32>] = [[1]];
    nested[0].push(2);
    copy[0].value + nested[0][1]
}
"#,
    );
}

#[test]
fn readonly_operations_cannot_recover_write_access() {
    let cases = [
        "fn main() { val a: [i32] = [1]; a.push(2); }",
        "fn main() { var a: [i32] = [1]; a[0] = 2; }",
        "fn main() { val a: [i32] = [1]; a[0] += 2; }",
        "fn main() { val a: [i32] = [1]; std::array::push(a, 2); }",
        "fn main() { val a: [i32] = [1]; set_index(a, 0, 2); }",
        "fn main() { val a = Map::from([(1, 2)]); a.insert(3, 4); }",
        "fn main() { val a = Map::from([(1, 2)]); std::map::clear(a); }",
        "fn main() { val a = Set::from([1]); a.remove(1); }",
        "fn main() { val a = Set::from([1]); std::set::clear(a); }",
        "fn main() { val a: [i32] = [1]; val b: MutableArray<i32> = a; }",
        "fn main() { val a = Map::from([(1, 2)]); val b: MutableMap<i32, i32> = a; }",
        "fn main() { val a = Set::from([1]); val b: MutableSet<i32> = a; }",
        "fn change<T>(a: MutableArray<T>, v: T) { a.push(v); } fn main() { val a: [i32] = [1]; change(a, 2); }",
        "fn bad(a: [i32]) -> MutableArray<i32> { a }",
        "struct Box { val a: MutableArray<i32> } fn main() { val a: [i32] = [1]; Box { a } }",
        "fn main() { val a = [[1]]; val b: MutableArray<Array<i32>> = a; }",
        "fn main() { val a = [[1]]; val b: Array<Array<i32>> = a; }",
        "fn main() { val a: [i32] = [1]; val change = || a.push(2); change(); }",
        "fn main() { val a: [i32] = [1]; val b = if true { a } else { [2] }; b.push(3); }",
        "fn main() { val a: [i32] = [1]; val b = match true { true => [2], false => a }; b.push(3); }",
        "fn main() { val a = [1]; a = [2]; }",
        "fn main() { val a: Map<i32, i32> = std::map::new(); }",
        "fn main() { val a: Set<i32> = std::set::new(); }",
    ];
    for source in cases {
        let result = KagariEngine::default().compile_to_artifact(
            SourceFile::new("readonly-negative.kgr", source),
            Default::default(),
            Default::default(),
        );
        assert!(result.is_err(), "must reject: {source}");
    }
}

#[test]
fn factories_use_custom_key_protocols_and_ordered_single_evaluation() {
    execute(
        r#"
struct Key { val id: i32 }
impl PartialEq for Key { fn eq(self, other: Self) -> bool { self.id == other.id } }
impl Eq for Key {}
impl Hash for Key { fn hash(self) -> i64 { self.id.hash() } }
struct Counter { var n: i32 }
fn next(c: Counter) -> i32 { c.n += 1; c.n }
fn main() -> i32 {
    val c = Counter { n: 0 };
    val first = Key { id: 1 };
    val second = Key { id: 1 };
    val entries = [(first, next(c)), (second, next(c))];
    val map = Map::from(entries);
    val mutable = MutableMap::from(entries);
    val set = Set::from([first, second]);
    val mutable_set = MutableSet::from([first, second]);
    std::debug::assert(map.get(first) == Some(2) && mutable.get(second) == Some(2), "last value wins");
    std::debug::assert(set.len() == [0].len() && mutable_set.len() == [0].len(), "custom dedup");
    std::debug::assert(c.n == 2, "evaluated once");
    val writable_copy = MutableArray::from(Array::from([40, 2]));
    writable_copy[0] + writable_copy[1]
}
"#,
    );
}

#[test]
fn associated_factories_resolve_qualified_names_and_function_aliases() {
    execute(
        r#"
use std::array as arrays;
use std::map::Map::from as map_of;
fn main() -> i32 {
    val array = arrays::MutableArray::from([20, 22]);
    val set = std::set::Set::from([20, 22]);
    val map = map_of([(1, array[0]), (2, array[1])]);
    std::debug::assert(set.contains(22), "qualified factory");
    map.get(1).unwrap_or(0) + map.get(2).unwrap_or(0)
}
"#,
    );
}

#[test]
fn factories_release_input_guards_after_callback_failure() {
    let source = r#"
struct Key { val input: MutableArray<Key> }
impl PartialEq for Key { fn eq(self, other: Self) -> bool { self === other } }
impl Eq for Key {}
impl Hash for Key { fn hash(self) -> i64 { self.input.push(self); 1.hash() } }
trait Test { fn attempt(self); fn clear(self) -> i32; }
struct Tester { val input: MutableArray<Key> }
impl Test for Tester {
    fn attempt(self) { Set::from(self.input); }
    fn clear(self) -> i32 { self.input.clear(); 42 }
}
fn make() -> Test {
    val input: MutableArray<Key> = [];
    input.push(Key { input });
    Tester { input }
}
"#;
    let mut config = kagari_embed::EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let engine = KagariEngine::new(config);
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("factory-cleanup.kgr", source),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    let artifact = BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    let module = &artifact.program.modules[artifact.program.root.index()];
    let declaration = module
        .trait_contracts
        .iter()
        .find(|c| c.abi.name == "Test")
        .unwrap()
        .declaration
        .clone();
    let method = |name: &str| {
        let mut id = declaration.clone();
        id.path
            .push(kagari_common::identity::DefinitionPathSegment {
                kind: kagari_common::identity::DefinitionKind::Method,
                name: name.into(),
                occurrence: 0,
            });
        id
    };
    let mut runtime_config = kagari_runtime::RuntimeConfig::default();
    runtime_config.gc.collection_threshold = Some(1);
    let mut runtime = kagari_runtime::Runtime::new(runtime_config);
    let loaded = runtime
        .load_program("factory-cleanup", artifact.program)
        .unwrap();
    let mut vm = kagari_vm::Vm::new(runtime);
    let input = vm.execute(&loaded, "make").unwrap().return_value;
    let root = vm.runtime().root_value(input.clone()).unwrap();
    let error = vm
        .invoke_interface_method(&input, &method("attempt"), &[])
        .unwrap_err();
    assert!(
        format!("{error:?}").contains("structural modification during iteration"),
        "{error:?}"
    );
    assert_eq!(vm.runtime().gc().active_roots(), 1);
    assert!(vm.runtime().execution_root().is_none());
    assert_eq!(
        vm.invoke_interface_method(&input, &method("clear"), &[])
            .unwrap(),
        Value::I32(42)
    );
    drop(root);
    assert_eq!(vm.runtime().gc().active_roots(), 0);
}

#[test]
fn forged_writes_and_access_upgrades_are_rejected_before_loading() {
    use kagari_common::collection::CollectionAccess;
    use kagari_hir::builtin::surface::StandardIntrinsic;
    use kagari_ir::bytecode::{BytecodeInstruction, CallTarget, verify_program};
    use kagari_ir::module::abi::AbiType;
    let engine = KagariEngine::default();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "access-wire.kgr",
                r#"
pub fn inspect(values: [i32]) { values.len(); }
fn main() { val values = Array::from([1, 2]); inspect(values); }
"#,
            ),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    verify_program(&artifact.program).unwrap();
    let root = artifact.program.root.index();
    let index = artifact.program.modules[root]
        .functions
        .iter()
        .position(|f| f.name == "inspect")
        .unwrap();
    let mut forged = artifact.program.clone();
    let function = &mut forged.modules[root].functions[index];
    let instruction = function
        .instructions
        .iter_mut()
        .find(|i| {
            matches!(
                i,
                BytecodeInstruction::Call {
                    callee: CallTarget::StandardIntrinsic(StandardIntrinsic::ArrayLen),
                    ..
                }
            )
        })
        .unwrap();
    if let BytecodeInstruction::Call { dst, callee, args } = instruction {
        *dst = Some(args[0]);
        *callee = CallTarget::StandardIntrinsic(StandardIntrinsic::ArrayClear);
    }
    let error = verify_program(&forged).unwrap_err();
    assert!(
        format!("{error:?}").contains("invalid collection access flow"),
        "{error:?}"
    );
    let mut forged = artifact.program.clone();
    let function = &mut forged.modules[root].functions[index];
    if let Some(AbiType::Array(_, access)) = function.metadata.semantic.params.get_mut(&0) {
        *access = CollectionAccess::Mutable;
    }
    assert!(verify_program(&forged).is_err());
    let mut forged = artifact.program.clone();
    forged.modules[root].functions[index]
        .metadata
        .semantic
        .params
        .clear();
    assert!(verify_program(&forged).is_err());
    let mut forged = artifact.clone();
    let main = forged.program.modules[root]
        .functions
        .iter_mut()
        .find(|f| f.name == "main")
        .unwrap();
    let ty = main
        .metadata
        .semantic
        .registers
        .values_mut()
        .find(|ty| matches!(ty, AbiType::Array(_, CollectionAccess::ReadOnly)))
        .unwrap();
    if let AbiType::Array(_, access) = ty {
        *access = CollectionAccess::Mutable;
    }
    assert!(forged.validate_for_loader(&Default::default()).is_err());
}

#[test]
fn host_results_preserve_declared_access_through_artifacts_and_binding_checks() {
    use kagari_common::{
        collection::CollectionAccess,
        host_interface::{HostFunctionDeclaration, HostInterface, HostValueType},
    };
    use kagari_runtime::host::HostFunction;
    let declaration = HostFunctionDeclaration::new(
        "demo.values",
        vec![],
        HostValueType::Array(Box::new(HostValueType::I32), CollectionAccess::ReadOnly),
    );
    let engine = KagariEngine::default();
    engine
        .set_host_interface(HostInterface {
            functions: vec![declaration.clone()],
            types: vec![],
            paths: vec![],
        })
        .unwrap();
    let profile = kagari_runtime::LanguageProfile {
        allow_host_calls: true,
        ..Default::default()
    };
    let options = kagari_embed::CompileOptions {
        language_profile: profile,
    };
    assert!(
        engine
            .compile_to_artifact(
                SourceFile::new("host-negative.kgr", "fn main() { demo::values().push(1); }"),
                options.clone(),
                Default::default()
            )
            .is_err()
    );
    let artifact = engine.compile_to_artifact(SourceFile::new("host-readonly.kgr", "fn main() -> i32 { val source = demo::values(); val copy = MutableArray::from(source); copy.push(2); copy[0] + copy[1] }"), options, Default::default()).unwrap();
    let artifact = BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    let mut context = ExecutionContext {
        language_profile: profile,
        ..Default::default()
    };
    context.capabilities.host_calls = true;
    context.host_policy.allowed_host_functions = vec!["demo.values".into()];
    let mut runtime = engine.runtime(context.clone());
    runtime
        .register_host_function(HostFunction::new(declaration.clone(), |context, _| {
            Ok(Value::Array(
                context.runtime().alloc_array(vec![Value::I32(40)]).unwrap(),
            ))
        }))
        .unwrap();
    let loaded = runtime
        .load_program(artifact.clone(), Default::default())
        .unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, "main", &[], &context)
            .unwrap()
            .return_value,
        Value::I32(42)
    );
    let mut changed = declaration;
    changed.return_type =
        HostValueType::Array(Box::new(HostValueType::I32), CollectionAccess::Mutable);
    let mut mismatched = engine.runtime(context);
    mismatched
        .register_host_function(HostFunction::new(changed, |context, _| {
            Ok(Value::Array(context.runtime().alloc_array(vec![]).unwrap()))
        }))
        .unwrap();
    assert!(
        mismatched
            .load_program(artifact, Default::default())
            .is_err()
    );
}

#[test]
fn factory_input_and_destination_survive_host_reentry_and_collection() {
    use kagari_common::host_interface::standard_log;
    use kagari_runtime::host::{HostError, HostFunction};
    use std::{cell::Cell, rc::Rc};
    let mut config = kagari_embed::EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let engine = KagariEngine::new(config);
    let mut context = ExecutionContext::default();
    context.language_profile.allow_host_calls = true;
    context.capabilities.host_calls = true;
    context.host_policy.allowed_host_functions = vec!["host.log".into()];
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "factory-reentry.kgr",
                r#"
struct Key { val id: i32 }
impl PartialEq for Key { fn eq(self, other: Self) -> bool { print("eq"); self.id == other.id } }
impl Eq for Key {}
impl Hash for Key { fn hash(self) -> i64 { print("hash"); 0.hash() } }
fn scratch() -> [i32] { Array::from([7, 8]) }
fn main() -> i32 {
    val first = Key { id: 1 };
    val second = Key { id: 2 };
    val map = Map::from([(first, 20), (second, 22)]);
    map.get(first).unwrap_or(0) + map.get(second).unwrap_or(0)
}
"#,
            ),
            kagari_embed::CompileOptions {
                language_profile: context.language_profile,
            },
            Default::default(),
        )
        .unwrap();
    let artifact = BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    let scratch = artifact.program.modules[artifact.program.root.index()]
        .functions
        .iter()
        .find(|f| f.name == "scratch")
        .unwrap()
        .id;
    let calls = Rc::new(Cell::new(0));
    let recorded = calls.clone();
    let mut runtime = engine.runtime(context.clone());
    runtime
        .register_host_function(HostFunction::new(standard_log(), move |call, _| {
            recorded.set(recorded.get() + 1);
            let root = call.runtime().execution_root().unwrap();
            let result = kagari_vm::reenter(call, &root, scratch, &[])
                .map_err(|error| HostError::new(format!("callback: {error:?}")))?;
            call.runtime().collect_garbage().unwrap();
            assert!(call.runtime().gc().validate_value(&result.value()));
            Ok(Value::Unit)
        }))
        .unwrap();
    let loaded = runtime.load_program(artifact, Default::default()).unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, "main", &[], &context)
            .unwrap()
            .return_value,
        Value::I32(42)
    );
    assert!(calls.get() >= 4);
    assert_eq!(runtime.runtime().gc().active_roots(), 0);
    assert!(runtime.runtime().execution_root().is_none());
}

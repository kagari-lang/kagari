use kagari_embed::{
    BytecodeArtifact,
    context::{ExecutionContext, JitPolicy},
    engine::{EngineConfig, KagariEngine},
    program::PreparedProgram,
};
use kagari_runtime::value::Value;
use kagari_source::source::SourceFile;
use kagari_types::{scalar::BuiltinType, ty::Ty};

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
fn constructors_and_live_views() {
    execute(
        r#"use std::collections::{HashMap, HashSet, Map, Set};

fn main() -> i32 {
    val a = Vec::from([1, 2]);
    val r: std::collections::List<i32> = a;
    a.push(3);
    if r.len() != 3usize { return 0; }
    val m: HashMap<String, i32> = HashMap::new();
    m.insert("a", 1); m.insert("a", 2);
    val mr: Map<String, i32> = m;
    if mr.get("a") != Some(2) { return 0; }
    m.insert("b", 3);
    if !mr.contains_key("b") { return 0; }
    val s: HashSet<i32> = HashSet::new();
    s.insert(1); s.insert(1); s.insert(2);
    if s.len() != 2usize { return 0; }
    val empty: Map<String, i32> = HashMap::new();
    val empty_array: Vec<i32> = Vec::new();
    val empty_set: Set<i32> = HashSet::new();
    if !(empty.is_empty() && empty_array.is_empty() && empty_set.is_empty()) { return 0; }
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
struct Shelf { val items: std::collections::List<Item> }
fn readable<T>(values: Vec<T>) -> std::collections::List<T> { values }
fn size<T>(values: std::collections::List<T>) -> usize { values.len() }
fn main() -> i32 {
    val item = Item { value: 1 };
    val writable = Vec::from([item]);
    val view = readable(writable);
    val shelf = Shelf { items: writable };
    shelf.items[0].value = 40;
    val copy = Vec::from([view[0]]);
    writable[0] = Item { value: 7 };
    if !(copy[0] === item && copy !== view) { return 0; }
    if !(view === writable && view == writable) { return 0; }
    if !(view.hash() == writable.hash()) { return 0; }
    val join = if true { view } else { writable };
    val other = match true { true => writable, false => view };
    val inspect = || size(join) == size(other);
    if !(inspect()) { return 0; }
    val nested: std::collections::List<Vec<i32>> = Vec::from([Vec::from([1])]);
    nested[0].push(2);
    copy[0].value + nested[0][1]
}
"#,
    );
}

#[test]
fn readonly_operations_cannot_recover_write_access() {
    let cases = [
        "fn main() { val a: std::collections::List<i32> = Vec::from([1]); a.push(2); }",
        "fn main() { var a: std::collections::List<i32> = Vec::from([1]); a[0] = 2; }",
        "fn main() { val a: std::collections::List<i32> = Vec::from([1]); a[0] += 2; }",
        "fn main() { val a: std::collections::List<i32> = Vec::from([1]); Vec::push(a, 2); }",
        "fn main() { val a: std::collections::List<i32> = Vec::from([1]); set_index(a, 0, 2); }",
        "use std::collections::{HashMap, Map};\nfn main() { val a: Map<i32, i32> = HashMap::new(); a.insert(3, 4); }",
        "use std::collections::{HashMap, Map};\nfn main() { val a: Map<i32, i32> = HashMap::new(); HashMap::clear(a); }",
        "use std::collections::{HashSet, Set};\nfn main() { val a: Set<i32> = HashSet::new(); a.remove(1); }",
        "use std::collections::{HashSet, Set};\nfn main() { val a: Set<i32> = HashSet::new(); HashSet::clear(a); }",
        "fn main() { val a: std::collections::List<i32> = Vec::from([1]); val b: Vec<i32> = a; }",
        "use std::collections::{HashMap, Map};\nfn main() { val a: Map<i32, i32> = HashMap::new(); val b: HashMap<i32, i32> = a; }",
        "use std::collections::{HashSet, Set};\nfn main() { val a: Set<i32> = HashSet::new(); val b: HashSet<i32> = a; }",
        "fn change<T>(a: Vec<T>, v: T) { a.push(v); } fn main() { val a: std::collections::List<i32> = Vec::from([1]); change(a, 2); }",
        "fn bad(a: std::collections::List<i32>) -> Vec<i32> { a }",
        "struct Box { val a: Vec<i32> } fn main() { val a: std::collections::List<i32> = Vec::from([1]); Box { a } }",
        "use std::collections::{List};\nfn main() { val a = Vec::from([Vec::from([1])]); val b: Vec<List<i32>> = a; }",
        "use std::collections::{List};\nfn main() { val a = Vec::from([Vec::from([1])]); val b: List<List<i32>> = a; }",
        "fn main() { val a: std::collections::List<i32> = Vec::from([1]); val change = || a.push(2); change(); }",
        "fn main() { val a: std::collections::List<i32> = Vec::from([1]); val b = if true { a } else { Vec::from([2]) }; b.push(3); }",
        "fn main() { val a: std::collections::List<i32> = Vec::from([1]); val b = match true { true => Vec::from([2]), false => a }; b.push(3); }",
        "fn main() { val a = Vec::from([1]); a = Vec::from([2]); }",
        "use std::collections::{Map};\nfn main() { val a: Map<i32, i32> = std::map::new(); }",
        "use std::collections::{Set};\nfn main() { val a: Set<i32> = std::set::new(); }",
    ];
    for source in cases {
        let result = KagariEngine::default().compile_to_artifact(
            SourceFile::new("readonly-negative.kgr", source),
            Default::default(),
        );
        assert!(result.is_err(), "must reject: {source}");
    }
}

#[test]
fn forged_access_upgrades_are_rejected_before_loading() {
    use kagari_bytecode::program::verify_program;
    let engine = KagariEngine::default();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "access-wire.kgr",
                r#"
pub fn inspect(values: std::collections::List<i32>) { values.len(); }
fn main() { val values = Vec::from([1, 2]); inspect(values); }
"#,
            ),
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
    if let Some(Ty::Trait(interface)) = function.metadata.semantic.params.get_mut(&0) {
        interface.declaration.path.last_mut().unwrap().name = "MutableList".into();
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
    let table = forged.program.modules.iter_mut().flat_map(|module| &mut module.public_items).find_map(|item| match item {
        kagari_contract::types::PublicItem::InterfaceTable(table) if matches!(&table.trait_type, Ty::Trait(interface) if interface.declaration.path.last().unwrap().name == "List") => Some(table),
        _ => None,
    }).unwrap();
    if let Ty::Trait(interface) = &mut table.trait_type {
        interface.declaration.path.last_mut().unwrap().name = "MutableList".into();
    }
    assert!(forged.validate_for_loader(&Default::default()).is_err());
}

#[test]
fn host_results_preserve_collection_families_through_artifacts_and_binding_checks() {
    use kagari_runtime::host::HostFunction;
    use kagari_types::host_interface::{
        HostFunctionDeclaration, HostInterface, value_type::HostValueType,
    };
    let declaration = HostFunctionDeclaration::new(
        "demo.values",
        vec![],
        HostValueType::Array(Box::new(HostValueType::I32)),
    );
    let engine = KagariEngine::default();
    engine
        .set_host_interface(HostInterface {
            functions: vec![declaration.clone()],
            types: vec![],
            paths: vec![],
        })
        .unwrap();

    assert!(
        engine
            .compile_to_artifact(
                SourceFile::new("host-negative.kgr", "fn main() { demo::values().push(1); }"),
                Default::default()
            )
            .is_err()
    );
    let artifact = engine.compile_to_artifact(SourceFile::new("host-readonly.kgr", "fn main() -> i32 { val source = demo::values(); val copy = Vec::from([source[0], 2]); copy[0] + copy[1] }"),  Default::default()).unwrap();
    let artifact = BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    let context = ExecutionContext {
        ..Default::default()
    };

    let mut runtime = engine.runtime(context.clone());
    runtime
        .register_host_function(HostFunction::new(declaration.clone(), |context, _| {
            Ok(Value::Array(
                context
                    .runtime()
                    .alloc_array(
                        &context.runtime().execution_root().unwrap(),
                        Ty::Builtin(BuiltinType::I32),
                        vec![Value::I32(40)],
                    )
                    .unwrap(),
            ))
        }))
        .unwrap();
    let loaded_program =
        PreparedProgram::from_artifact(artifact.clone(), &Default::default(), &Default::default())
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
    let mut changed = declaration;
    let Ty::NativeObject(vector) = kagari_stdlib::declarations::StandardDeclarations::default()
        .vec(kagari_runtime::native::types::Type::i32())
        .abi()
        .clone()
    else {
        panic!("nominal Vec");
    };
    changed.return_type = HostValueType::Vec(vector.declaration, Box::new(HostValueType::I32));
    let mut mismatched = engine.runtime(context);
    mismatched
        .register_host_function(HostFunction::new(changed, |context, _| {
            Ok(Value::Array(
                context
                    .runtime()
                    .alloc_array(
                        &context.runtime().execution_root().unwrap(),
                        Ty::Builtin(BuiltinType::I32),
                        vec![],
                    )
                    .unwrap(),
            ))
        }))
        .unwrap();
    assert!(
        mismatched
            .load_program(
                &PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                    .unwrap(),
                Default::default()
            )
            .is_err()
    );
}

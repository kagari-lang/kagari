use crate::test_support::execution;
use execution::Route;
use kagari_runtime::module::entry::ProgramEntry;

use crate::{
    BytecodeArtifact,
    context::ExecutionContext,
    engine::{EngineConfig, KagariEngine},
    program::PreparedProgram,
};
use kagari_bytecode::program::{identity, verify_program};
use kagari_common::identity::{map::DefinitionContext, table::DefinitionId};
use kagari_contract::types::PublicItem;
use kagari_runtime::{
    host::HostCallContext,
    native::{builder::ModuleBuilder, declarations::FunctionDecl, types::Type},
    value::Value,
};
use kagari_source::source::SourceFile;
use kagari_stdlib::declarations::StandardDeclarations;
use kagari_types::{
    collection::CollectionAccess,
    declaration::host::{HostCallPolicy, HostFunctionEffects, HostPassingStyle},
    scalar::BuiltinType,
    ty::Ty,
};

fn execute(source: &str) {
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let engine = execution::engine(config);
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("standard-traits.kgr", source),
            Default::default(),
        )
        .unwrap();
    execution::assert_returns_i32(
        &engine,
        artifact,
        &[Route::Source, Route::Artifact, Route::ArtifactJit],
        42,
    );
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
    if !(copy[0] === item && copy !== writable) { return 0; }
    if !(view[0] === writable[0] && view[0].value == 7) { return 0; }
    val join: std::collections::List<Item> = if true { view } else { writable };
    val other: std::collections::List<Item> = match true { true => writable, false => view };
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
    let definitions = DefinitionContext::new().unwrap();
    let artifact = artifact
        .import_into(&definitions, &Default::default())
        .unwrap();
    identity::collect(
        &artifact
            .records()
            .program
            .modules
            .iter()
            .collect::<Vec<_>>(),
        &definitions,
    )
    .unwrap();
    verify_program(&artifact.records().program, &definitions.snapshot()).unwrap();
    let root = artifact.records().program.root.index();
    let index = artifact.records().program.modules[root]
        .functions
        .iter()
        .position(|f| f.name == "inspect")
        .unwrap();
    let mut forged = artifact.records().program.clone();
    let function = &mut forged.modules[root].functions[index];
    if let Some(Ty::Trait(interface)) = function.metadata.semantic.params.get_mut(&0) {
        interface.declaration = mutable_list(&definitions, interface.declaration);
    }
    assert!(verify_program(&forged, &definitions.snapshot()).is_err());
    let mut forged = artifact.records().program.clone();
    forged.modules[root].functions[index]
        .metadata
        .semantic
        .params
        .clear();
    assert!(verify_program(&forged, &definitions.snapshot()).is_err());
    let mut forged = artifact.into_records();
    let table = forged.program.modules.iter_mut().flat_map(|module| &mut module.public_items).find_map(|item| match item {
        PublicItem::InterfaceTable(table) if matches!(&table.trait_type, Ty::Trait(interface) if definitions.snapshot().resolve(interface.declaration).unwrap().last().unwrap().name == "List") => Some(table),
        _ => None,
    }).unwrap();
    if let Ty::Trait(interface) = &mut table.trait_type {
        interface.declaration = mutable_list(&definitions, interface.declaration);
    }
    assert!(
        forged
            .validate_for_loader(&Default::default(), &definitions)
            .is_err()
    );
}

#[test]
fn boxed_interface_dispatch_requires_an_interface_value_without_collection_markers() {
    let engine = KagariEngine::default();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "ordinary-interface-receiver.kgr",
                r#"
trait Reader { fn read(self) -> i32; }
pub fn inspect(value: Reader) -> i32 { value.read() }
fn main() -> i32 { 42 }
"#,
            ),
            Default::default(),
        )
        .unwrap();
    let context = DefinitionContext::new().unwrap();
    let artifact = artifact.import_into(&context, &Default::default()).unwrap();
    identity::collect(
        &artifact
            .records()
            .program
            .modules
            .iter()
            .collect::<Vec<_>>(),
        &context,
    )
    .unwrap();
    verify_program(&artifact.records().program, &context.snapshot()).unwrap();
    let mut program = artifact.records().program.clone();
    let root = program.root.index();
    let function = program.modules[root]
        .functions
        .iter_mut()
        .find(|function| function.name == "inspect")
        .unwrap();
    let original = function.metadata.semantic.params[&0].clone();
    let replacement = Ty::Tuple(vec![Ty::Builtin(BuiltinType::I32)]);
    let semantic = &mut function.metadata.semantic;
    for ty in semantic
        .params
        .values_mut()
        .chain(semantic.locals.values_mut())
        .chain(semantic.registers.values_mut())
    {
        if *ty == original {
            *ty = replacement.clone();
        }
    }
    let declaration = program.modules[root]
        .public_items
        .iter_mut()
        .find_map(|item| match item {
            PublicItem::Function(function) if function.name == "inspect" => Some(function),
            _ => None,
        })
        .unwrap();
    declaration.params[0].ty = replacement;
    assert!(
        verify_program(&program, &context.snapshot()).is_err(),
        "a raw heap value cannot satisfy boxed-interface dispatch by sharing its physical representation"
    );
}

#[test]
fn host_results_preserve_collection_families_through_artifacts_and_binding_checks() {
    let declarations = StandardDeclarations::default();
    let module = |result: Type| {
        let mut builder = ModuleBuilder::new("demo", &declarations.catalog().unwrap()).unwrap();
        let values = builder
            .define_function(FunctionDecl::new("values").returns(result).host_policy(
                HostCallPolicy {
                    passing: vec![],
                    effects: HostFunctionEffects {
                        may_allocate: true,
                        ..Default::default()
                    },
                },
            ))
            .unwrap();
        builder
            .bind(values, |context: &HostCallContext<'_>, _: &[Value]| {
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
            })
            .unwrap();
        builder.finish().unwrap()
    };
    let array = Type::from_semantic(
        Ty::Array(Box::new(Ty::Builtin(BuiltinType::I32))),
        &DefinitionContext::new().unwrap().snapshot(),
    )
    .unwrap();
    let engine =
        KagariEngine::with_native_modules(Default::default(), vec![module(array)]).unwrap();

    assert!(
        engine
            .compile_to_artifact(
                SourceFile::new(
                    "host-negative.kgr",
                    "use demo; fn main() { demo::values().push(1); }"
                ),
                Default::default()
            )
            .is_err()
    );
    let artifact = engine.compile_to_artifact(SourceFile::new("host-readonly.kgr", "use demo; fn main() -> i32 { val source = demo::values(); val copy = Vec::from([source[0], 2]); copy[0] + copy[1] }"),  Default::default()).unwrap();
    let artifact =
        BytecodeArtifact::from_bytes(&artifact.records().to_bytes(artifact.definitions()).unwrap())
            .unwrap();
    let context = ExecutionContext {
        ..Default::default()
    };

    let mut runtime = engine.runtime(context.clone());
    let loaded_program =
        PreparedProgram::from_artifact(artifact.clone(), &Default::default(), &Default::default())
            .unwrap();
    let loaded = runtime
        .load_program(&loaded_program, Default::default())
        .unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, ProgramEntry::Main, &[], &context)
            .unwrap()
            .return_value
            .value(runtime.runtime().gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
    let changed = module(declarations.vec(Type::i32()).unwrap());
    let changed = KagariEngine::with_native_modules(Default::default(), vec![changed]).unwrap();
    let mut mismatched = changed.runtime(context);
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

fn mutable_list(definitions: &DefinitionContext, id: DefinitionId) -> DefinitionId {
    let table = definitions.snapshot();
    let parent = table.parent(id).unwrap().unwrap();
    let segment = table.resolve(id).unwrap().last().unwrap();
    definitions
        .intern_child(parent, segment.kind, "MutableList", segment.occurrence)
        .unwrap()
}

#[test]
fn readonly_host_hash_storage_has_exact_read_and_content_protocols() {
    let catalog = StandardDeclarations::default().catalog().unwrap();
    let mut builder = ModuleBuilder::new("demo", &catalog).unwrap();
    let map = |access| Type::map(Type::i32(), Type::i32(), access).unwrap();
    let set = |access| Type::set(Type::i32(), access).unwrap();
    for (name, input, output) in [
        (
            "read_map",
            map(CollectionAccess::Mutable),
            map(CollectionAccess::ReadOnly),
        ),
        (
            "read_set",
            set(CollectionAccess::Mutable),
            set(CollectionAccess::ReadOnly),
        ),
    ] {
        let function = builder
            .define_function(
                FunctionDecl::new(name)
                    .parameter("value", input)
                    .returns(output)
                    .host_policy(HostCallPolicy {
                        passing: vec![HostPassingStyle::Owned],
                        effects: Default::default(),
                    }),
            )
            .unwrap();
        builder
            .bind(function, |_: &HostCallContext<'_>, args: &[Value]| {
                Ok(args[0])
            })
            .unwrap();
    }
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let engine =
        KagariEngine::with_native_modules(config, vec![builder.finish().unwrap()]).unwrap();
    for expression in [
        "val raw: HashMap<i32, i32> = HashMap::new(); val ro = demo::read_map(raw); ro.insert(1, 2);",
        "val ro = demo::read_set(HashSet::from([1])); ro.insert(2);",
        "val raw: HashMap<i32, i32> = HashMap::new(); val ro = demo::read_map(raw); val writable: MutableMap<i32, i32> = ro;",
        "val ro = demo::read_set(HashSet::from([1])); val writable: MutableSet<i32> = ro;",
    ] {
        let source = format!(
            "use demo; use std::collections::{{HashMap, HashSet, MutableMap, MutableSet}}; fn main() {{ {expression} }}"
        );
        assert!(
            engine
                .compile_to_artifact(
                    SourceFile::new("host-readonly-negative.kgr", source),
                    Default::default()
                )
                .is_err()
        );
    }
    let source = r#"
use demo;
use std::collections::{HashMap, HashSet, Map, Set, map_content_equal, set_content_equal};
use std::hash::Hash;
fn equal<T: Eq>(a: T, b: T) -> bool { a == b }
fn main() -> i32 {
    val a: HashMap<i32, i32> = HashMap::new(); a.insert(1, 40);
    val b: HashMap<i32, i32> = HashMap::new(); b.insert(1, 40);
    val x = demo::read_map(a); val y = demo::read_map(b);
    if !(equal(x, y) && x.hash() == y.hash() && x.get(1) == Some(40)) { return 1; }
    val mx: Map<i32, i32> = x; val my: Map<i32, i32> = y;
    if !(x === a && map_content_equal(mx, my)) { return 2; }
    val sa = HashSet::from([1, 2]); val sb = HashSet::from([2, 1]);
    val sx = demo::read_set(sa); val sy = demo::read_set(sb);
    if !(equal(sx, sy) && sx.hash() == sy.hash() && sx.contains(2)) { return 3; }
    val ix: Set<i32> = sx; val iy: Set<i32> = sy;
    if !(sx === sa && set_content_equal(ix, iy)) { return 4; }
    a.insert(2, 2); sa.insert(3);
    if !(mx.get(2) == Some(2) && ix.contains(3)) { return 5; }
    42
}
"#;
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("host-readonly-protocols.kgr", source),
            Default::default(),
        )
        .unwrap();
    let decoded =
        BytecodeArtifact::from_bytes(&artifact.records().to_bytes(artifact.definitions()).unwrap())
            .unwrap();
    let program =
        PreparedProgram::from_artifact(decoded, &Default::default(), &Default::default()).unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    let result = runtime
        .execute(&loaded, ProgramEntry::Main, &[], &context)
        .unwrap();
    assert_eq!(
        result.return_value.value(runtime.runtime().gc()),
        Some(Value::I32(42))
    );
    drop(result);
    assert_eq!(runtime.runtime().gc().active_roots(), 0);
    assert_eq!(runtime.runtime().collect_garbage().unwrap().live_objects, 0);
}

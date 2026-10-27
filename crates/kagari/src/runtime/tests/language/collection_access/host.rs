use crate::{
    BytecodeArtifact,
    context::ExecutionContext,
    engine::{EngineConfig, KagariEngine},
    program::PreparedProgram,
};
use kagari_common::identity::map::DefinitionContext;
use kagari_runtime::{
    host::HostCallContext,
    module::entry::ProgramEntry,
    native::{builder::ModuleBuilder, declarations::FunctionDecl, types::Type},
    value::Value,
};
use kagari_source::source::SourceFile;
use kagari_stdlib::declarations::StandardDeclarations;
use kagari_types::{
    declaration::host::{HostCallPolicy, HostFunctionEffects, HostPassingStyle},
    scalar::BuiltinType,
    ty::Ty,
};

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

#[test]
fn readonly_host_hash_storage_has_exact_read_and_content_protocols() {
    let catalog = StandardDeclarations::default().catalog().unwrap();
    let mut builder = ModuleBuilder::new("demo", &catalog).unwrap();
    let standard = StandardDeclarations::default();
    let map = standard.hash_map(Type::i32(), Type::i32()).unwrap();
    let set = standard.hash_set(Type::i32()).unwrap();
    let map_view = catalog
        .trait_reference(&kagari_stdlib::identity::trait_id("Map"))
        .unwrap()
        .apply([Type::i32(), Type::i32()])
        .unwrap();
    let set_view = catalog
        .trait_reference(&kagari_stdlib::identity::trait_id("Set"))
        .unwrap()
        .apply([Type::i32()])
        .unwrap();
    for (name, input, output) in [("read_map", map, map_view), ("read_set", set, set_view)] {
        let function = builder
            .define_function(
                FunctionDecl::new(name)
                    .parameter("value", input.clone())
                    .returns(output.ty())
                    .host_policy(HostCallPolicy {
                        passing: vec![HostPassingStyle::Owned],
                        effects: Default::default(),
                    }),
            )
            .unwrap();
        builder
            .function(&function, |function| function.produces(input))
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
    if !(equal(a, b) && a.hash() == b.hash() && x.get(1) == Some(40)) { return 1; }
    val mx: Map<i32, i32> = x; val my: Map<i32, i32> = y;
    if !map_content_equal(mx, my) { return 2; }
    val sa = HashSet::from([1, 2]); val sb = HashSet::from([2, 1]);
    val sx = demo::read_set(sa); val sy = demo::read_set(sb);
    if !(equal(sa, sb) && sa.hash() == sb.hash() && sx.contains(2)) { return 3; }
    val ix: Set<i32> = sx; val iy: Set<i32> = sy;
    if !set_content_equal(ix, iy) { return 4; }
    a.insert(2, 2); sa.insert(3);
    if !(x.get(2) == Some(2) && mx.get(2) == Some(2) && y.get(2) == None
        && sx.contains(3) && ix.contains(3) && !sy.contains(3)) { return 5; }
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

use super::{compile, compile_program};
use kagari_abi::types::ConcreteFunctionIdentity;
use kagari_bytecode::{
    artifact::KbcArtifact, program::verify_program, verifier::BytecodeVerificationError,
};
use kagari_runtime::{
    Runtime,
    native::{
        binding::NativeResult, builder::ModuleBuilder, context::CallContext,
        declarations::FunctionDecl, language::LanguageContracts, types::Type,
    },
    value::Value,
};
use kagari_vm::vm::Vm;

#[test]
fn dynamic_list_iteration_adapts_the_concrete_cursor_result() {
    let (mut vm, loaded) = compile(
        r#"
        fn sum(values: List<i32>) -> i32 {
            var result = 0;
            for value in values { result += value; }
            result
        }
        fn main() -> i32 { sum([20, 22]) }
        "#,
        None,
    );
    assert_eq!(
        vm.execute(&loaded, "main").unwrap().return_value,
        Value::I32(42)
    );
}

#[test]
fn dynamic_iterator_adapters_round_trip_and_reject_forged_tables() {
    let program = compile_program(
        "fn sum(values: List<i32>) -> i32 { var n = 0; for x in values { n += x; } n } fn main() -> i32 { sum([20, 22]) }",
        None,
    );
    let artifact = KbcArtifact::from_program(program.clone(), Default::default()).unwrap();
    let decoded = KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    let mut runtime = Runtime::default();
    let loaded = runtime.load_program("round-trip", decoded.program).unwrap();
    assert_eq!(
        Vm::new(runtime)
            .execute(&loaded, "main")
            .unwrap()
            .return_value,
        Value::I32(42)
    );
    let (owner, table) = program
        .modules
        .iter()
        .enumerate()
        .find_map(|(owner, module)| {
            module
                .interface_tables
                .iter()
                .position(|table| table.view.is_some())
                .map(|table| (owner, table))
        })
        .unwrap();
    for corruption in 0..5 {
        let mut forged = program.clone();
        let table = &mut forged.modules[owner].interface_tables[table];
        let identity = ConcreteFunctionIdentity {
            declaration: table.declaration.clone(),
            arguments: table.arguments.clone(),
        };
        let view = table.view.as_mut().unwrap();
        match corruption {
            0 => view.results.clear(),
            1 => view.results[0].implementation = identity,
            2 => view.results.push(view.results[0].clone()),
            3 => view.interface.associated_types.clear(),
            _ => table.view = None,
        }
        assert!(
            matches!(
                verify_program(&forged),
                Err(BytecodeVerificationError::InvalidInterfaceTable)
            ),
            "corruption {corruption}"
        );
    }
}

#[test]
fn dynamic_iteration_preserves_gc_roots_and_mutation_guards() {
    let mut builder = ModuleBuilder::new("example::collection_gc", &LanguageContracts::default());
    let collect = builder
        .define_function(FunctionDecl::new("collect").returns(Type::unit()))
        .unwrap();
    builder
        .bind(collect, |cx: &mut CallContext<'_>| -> NativeResult<()> {
            cx.collect_garbage()?;
            Ok(())
        })
        .unwrap();
    let module = builder.finish().unwrap();
    let (mut vm, loaded) = compile(
        r#"
        use example::collection_gc::collect;
        struct Node { val value: i32 }
        fn sum(values: MutableList<Node>) -> i32 {
            val view: List<Node> = values;
            var sum = 0;
            for node in view { collect(); sum += node.value; }
            sum
        }
        fn main() -> i32 { sum([Node { value: 20 }, Node { value: 22 }]) }
        fn mutate(values: MutableList<i32>) {
            val view: List<i32> = values;
            for item in view { values.push(item); }
        }
        fn trap() { mutate([1, 2]); }
    "#,
        Some(&module),
    );
    assert_eq!(
        vm.execute(&loaded, "main").unwrap().return_value,
        Value::I32(42)
    );
    assert!(vm.execute(&loaded, "trap").is_err());
    assert_eq!(
        vm.execute(&loaded, "main").unwrap().return_value,
        Value::I32(42)
    );
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
}

#[test]
fn script_iterator_results_use_the_same_dynamic_adapter() {
    let (mut vm, loaded) = compile(
        r#"
        struct Cursor { var current: i32 }
        impl Iterator for Cursor {
            type Item = i32;
            fn next(self) -> Option<i32> {
                if self.current < 2 { self.current += 1; Some(21) } else { None }
            }
        }
        struct Items {}
        impl Iterable for Items {
            type Item = i32;
            type Iter = Cursor;
            fn iter(self) -> Cursor { Cursor { current: 0 } }
        }
        trait Bag: Iterable<Item = i32> {}
        impl Bag for Items {}
        fn sum(values: Bag) -> i32 {
            var result = 0;
            for item in values { result += item; }
            result
        }
        fn main() -> i32 { sum(Items {}) }
    "#,
        None,
    );
    assert_eq!(
        vm.execute(&loaded, "main").unwrap().return_value,
        Value::I32(42)
    );
}

#[test]
fn default_map_and_set_iterate_through_readonly_parent_views() {
    let (mut vm, loaded) = compile(
        r#"
        fn map_sum(values: MutableMap<i32, i32>) -> i32 {
            val view: Map<i32, i32> = values;
            var result = 0;
            for (key, value) in view { result += key + value; }
            result
        }
        fn set_sum(values: MutableSet<i32>) -> i32 {
            val view: Set<i32> = values;
            var result = 0;
            for value in view { result += value; }
            result
        }
        fn main() -> i32 {
            val map: HashMap<i32, i32> = HashMap::new();
            map.insert(1, 19);
            val set: HashSet<i32> = HashSet::new();
            set.insert(10); set.insert(12);
            map_sum(map) + set_sum(set)
        }
    "#,
        None,
    );
    assert_eq!(
        vm.execute(&loaded, "main").unwrap().return_value,
        Value::I32(42)
    );
}

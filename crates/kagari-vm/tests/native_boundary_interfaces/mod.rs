use super::{compile, compile_program};
use kagari_bytecode::{
    artifact::KbcArtifact, program::verify_program, verifier::BytecodeVerificationError,
};
use kagari_contract::types::ConcreteFunctionIdentity;
use kagari_runtime::{
    Runtime,
    error::RuntimeError,
    native::{
        binding::{Codec, NativeBinding, NativeResult},
        builder::ModuleBuilder,
        context::CallContext,
        declarations::{CallableRequirement, FunctionDecl, MethodDecl},
        module::NativeModule,
        types::Type,
    },
    value::Value,
};
use kagari_stdlib::declarations::StandardDeclarations;
use kagari_vm::vm::Vm;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[test]
fn generic_native_default_uses_the_interface_callers_type_arguments() {
    let mut module = ModuleBuilder::new(
        "example::generic_defaults",
        &StandardDeclarations::default()
            .catalog()
            .expect("explicit standard providers"),
    );
    let mut declaration = module.define_trait("Identity");
    declaration.type_parameter("Item").unwrap();
    let identity = declaration
        .define_method(MethodDecl::instance("identity"))
        .unwrap();
    declaration
        .method(&identity, |method| {
            let value = method.type_parameter("T")?.ty();
            method.parameter("value", value.clone());
            method.returns(value);
            Ok(())
        })
        .unwrap();
    declaration
        .bind_default_with(
            identity,
            NativeBinding::new(vec![Codec::Value, Codec::Value], Codec::Value, |cx| {
                cx.argument(1)
            }),
        )
        .unwrap();
    let contract = declaration.finish().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    module
        .implement(Type::i32(), |group| {
            group.trait_impl(contract.apply([Type::i32()]), |methods| {
                methods.bind_with(
                    "identity",
                    NativeBinding::new(vec![Codec::Value, Codec::Value], Codec::Value, move |cx| {
                        observed.fetch_add(1, Ordering::SeqCst);
                        cx.argument(1)
                    }),
                )
            })
        })
        .unwrap();
    let module = module.finish().unwrap();
    let program = compile_program(
        r#"
        use example::generic_defaults::Identity;
        struct Number {}
        struct Item { val value: i32 }
        impl Identity<i32> for Number {}
        fn main() -> i32 {
            val source: Identity<i32> = Number {};
            val value: Item = source.identity(Item { value: 21 });
            value.value + source.identity(21)
        }
        fn overridden() -> i32 { val source: Identity<i32> = 1; source.identity(42) }
        struct Holder<T> { val value: T }
        impl<T> Identity<Option<T>> for Holder<T> {}
        trait Relay {
            fn relay<T>(self, value: T) -> T {
                val concrete = Holder { value };
                val first = concrete.identity(value);
                val source: Identity<Option<T>> = concrete;
                source.identity(first)
            }
        }
        impl Relay for i32 {}
        fn nested() -> i32 {
            val relay: Relay = 0;
            relay.relay(Item { value: 42 }).value
        }
        trait BoundedRelay {
            fn invoke<S: Identity<i32>, T>(self, source: S, value: T) -> T {
                source.identity(value)
            }
        }
        impl BoundedRelay for i32 {}
        fn bounded() -> i32 {
            val relay: BoundedRelay = 0;
            val value = relay.invoke(Number {}, Item { value: 21 });
            relay.invoke(0, value).value * 2
        }
    "#,
        Some(&module),
    );
    for mutation in 0..3 {
        let mut forged = program.clone();
        let import = forged
            .modules
            .iter_mut()
            .flat_map(|owner| &mut owner.native_imports)
            .find(|import| import.generic.is_some())
            .unwrap();
        match mutation {
            0 => import.generic = None,
            1 => import.generic.as_mut().unwrap().parameters[0].position += 1,
            _ => import.signature.result = Type::bool().abi().clone(),
        }
        assert!(
            verify_program(&forged).is_err(),
            "native mutation {mutation}"
        );
    }
    let artifact = KbcArtifact::from_program(program, Default::default()).unwrap();
    let decoded = KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    let mut runtime = Runtime::default();
    kagari_runtime::native::module::NativeModule::install_all(
        &kagari_stdlib::modules().unwrap(),
        &mut runtime,
    )
    .unwrap();
    module.install(&mut runtime).unwrap();
    let loaded = runtime
        .load_program("shared-native", decoded.program)
        .unwrap();
    let vm = Vm::new(runtime);
    assert_eq!(
        vm.execute(&loaded, "main")
            .unwrap()
            .return_value
            .value(vm.runtime().gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
    assert_eq!(
        vm.execute(&loaded, "overridden")
            .unwrap()
            .return_value
            .value(vm.runtime().gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        vm.execute(&loaded, "nested")
            .unwrap()
            .return_value
            .value(vm.runtime().gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
    assert_eq!(
        vm.execute(&loaded, "bounded")
            .unwrap()
            .return_value
            .value(vm.runtime().gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn registered_default_calls_the_selected_receiver_operation() {
    let mut module = ModuleBuilder::new(
        "example::defaults",
        &StandardDeclarations::default()
            .catalog()
            .expect("explicit standard providers"),
    );
    let mut declaration = module.define_trait("Read");
    let read = declaration
        .define_method(MethodDecl::instance("read").returns(Type::i32()))
        .unwrap();
    let twice = declaration
        .define_method(MethodDecl::instance("twice").returns(Type::i32()))
        .unwrap();
    let operation = declaration.operation(&read).unwrap();
    let selected = declaration
        .method(&twice, |method| Ok(method.requires(operation)))
        .unwrap();
    declaration
        .bind_default_with(
            twice,
            NativeBinding::new(
                vec![Codec::Value],
                Codec::Scalar(Type::i32().abi().clone()),
                move |cx| {
                    let receiver = cx.argument(0)?;
                    let target = cx.selected(&selected)?;
                    let Value::I32(value) = cx.call_values(target, &[receiver])? else {
                        return Err(RuntimeError::module_validation("checked Read result"));
                    };
                    Ok(Value::I32(value * 2))
                },
            ),
        )
        .unwrap();
    declaration.finish().unwrap();
    let module = module.finish().unwrap();
    let (vm, loaded) = compile(
        r#"
        use example::defaults::Read;
        struct Number {}
        impl Read for Number { fn read(self) -> i32 { 21 } }
        fn main() -> i32 { Number {}.twice() }
        fn dynamic(value: Read) -> i32 { value.twice() }
        fn run_dynamic() -> i32 { dynamic(Number {}) }
        "#,
        Some(&module),
    );
    for entry in ["main", "run_dynamic"] {
        assert_eq!(
            vm.execute(&loaded, entry)
                .unwrap()
                .return_value
                .value(vm.runtime().gc())
                .expect("retained execution result"),
            Value::I32(42)
        );
    }
}

#[test]
fn dynamic_list_iteration_adapts_the_concrete_cursor_result() {
    let (vm, loaded) = compile(
        r#"use std::collections::{List};

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
        vm.execute(&loaded, "main")
            .unwrap()
            .return_value
            .value(vm.runtime().gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
}

#[test]
fn dynamic_iterator_adapters_round_trip_and_reject_forged_tables() {
    let program = compile_program(
        "use std::collections::{List};\nfn sum(values: List<i32>) -> i32 { var n = 0; for x in values { n += x; } n } fn main() -> i32 { sum([20, 22]) }",
        None,
    );
    let artifact = KbcArtifact::from_program(program.clone(), Default::default()).unwrap();
    let decoded = KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    let mut runtime = Runtime::default();
    kagari_runtime::native::module::NativeModule::install_all(
        &kagari_stdlib::modules().unwrap(),
        &mut runtime,
    )
    .unwrap();
    let loaded = runtime.load_program("round-trip", decoded.program).unwrap();
    let vm = Vm::new(runtime);
    assert_eq!(
        vm.execute(&loaded, "main")
            .unwrap()
            .return_value
            .value(vm.runtime().gc())
            .expect("retained execution result"),
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
    let mut builder = ModuleBuilder::new(
        "example::collection_gc",
        &StandardDeclarations::default()
            .catalog()
            .expect("explicit standard providers"),
    );
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
    let (vm, loaded) = compile(
        r#"use std::collections::{List, MutableList};

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
        vm.execute(&loaded, "main")
            .unwrap()
            .return_value
            .value(vm.runtime().gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
    assert!(vm.execute(&loaded, "trap").is_err());
    assert_eq!(
        vm.execute(&loaded, "main")
            .unwrap()
            .return_value
            .value(vm.runtime().gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
}

#[test]
fn script_iterator_results_use_the_same_dynamic_adapter() {
    let (vm, loaded) = compile(
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
        vm.execute(&loaded, "main")
            .unwrap()
            .return_value
            .value(vm.runtime().gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
}

#[test]
fn default_map_and_set_iterate_through_readonly_parent_views() {
    let (vm, loaded) = compile(
        r#"use std::collections::{HashMap, HashSet, Map, MutableMap, MutableSet, Set};

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
        vm.execute(&loaded, "main")
            .unwrap()
            .return_value
            .value(vm.runtime().gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
}

#[test]
fn generic_native_default_uses_the_callers_ordering_operation() {
    let language = StandardDeclarations::default();
    let mut module = ModuleBuilder::new(
        "example::comparison",
        &language.catalog().expect("explicit standard providers"),
    );
    let mut declaration = module.define_trait("Compare");
    let compare = declaration
        .define_method(MethodDecl::instance("compare").returns(language.ordering()))
        .unwrap();
    let selected = declaration
        .method(&compare, |method| {
            let key = method.type_parameter("K")?.ty();
            method.parameter("left", key.clone());
            method.parameter("right", key.clone());
            method.bound(key.clone(), language.ord().apply([]));
            Ok(method.requires(CallableRequirement::method(
                key,
                language.ord().method("cmp")?,
            )))
        })
        .unwrap();
    declaration
        .bind_default_with(
            compare,
            NativeBinding::new(
                vec![Codec::Value, Codec::Value, Codec::Value],
                Codec::Value,
                move |cx| {
                    let args = [cx.argument(1)?, cx.argument(2)?];
                    let target = cx.selected(&selected)?;
                    cx.call_values(target, &args)
                },
            ),
        )
        .unwrap();
    declaration.finish().unwrap();
    let module = module.finish().unwrap();
    let program = compile_program(
        r#"use std::cmp::{Ordering};

        use example::comparison::Compare;
        struct Source {}
        impl Compare for Source {}
        struct Rank { val value: i32 }
        impl PartialEq for Rank { fn eq(self, other: Self) -> bool { self.value == other.value } }
        impl Eq for Rank {}
        impl PartialOrd for Rank { fn partial_cmp(self, other: Self) -> Option<Ordering> { self.value.partial_cmp(other.value) } }
        impl Ord for Rank { fn cmp(self, other: Self) -> Ordering { other.value.cmp(self.value) } }
        fn main() -> i32 {
            val source: Compare = Source {};
            if source.compare(10, 20) != Ordering::Less { return 0; };
            val left = Rank { value: 10 };
            val right = Rank { value: 20 };
            if source.compare(left, right) != Ordering::Greater { return 0; };
            if source.compare("a", "b") == Ordering::Less { 42 } else { 0 }
        }
    "#,
        Some(&module),
    );
    let artifact = KbcArtifact::from_program(program, Default::default()).unwrap();
    let decoded = KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    let mut runtime = Runtime::default();
    kagari_runtime::native::module::NativeModule::install_all(
        &kagari_stdlib::modules().unwrap(),
        &mut runtime,
    )
    .unwrap();
    module.install(&mut runtime).unwrap();
    let loaded = runtime
        .load_program("native-constraint", decoded.program)
        .unwrap();
    let vm = Vm::new(runtime);
    assert_eq!(
        vm.execute(&loaded, "main")
            .unwrap()
            .return_value
            .value(vm.runtime().gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
}

#[test]
fn a_shared_method_calls_an_ordinary_generic_native_function() {
    let module = generic_identity_module();
    let program = compile_program(
        r#"
use example::generic_helper::identity;
trait Forward { fn forward<T>(self, value: T) -> T { identity(value) } }
struct Source {}
impl Forward for Source {}
struct Item { val value: i32 }
fn main() -> i32 {
    val source: Forward = Source {};
    source.forward(21) + source.forward(Item { value: 21 }).value
}
"#,
        Some(&module),
    );
    let artifact = KbcArtifact::from_program(program, Default::default()).unwrap();
    let decoded = KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    let mut runtime = Runtime::default();
    kagari_runtime::native::module::NativeModule::install_all(
        &kagari_stdlib::modules().unwrap(),
        &mut runtime,
    )
    .unwrap();
    module.install(&mut runtime).unwrap();
    let loaded = runtime
        .load_program("shared-helper", decoded.program)
        .unwrap();
    let vm = Vm::new(runtime);
    assert_eq!(
        vm.execute(&loaded, "main")
            .unwrap()
            .return_value
            .value(vm.runtime().gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
}

pub(super) fn generic_identity_module() -> NativeModule {
    let language = StandardDeclarations::default();
    let mut module = ModuleBuilder::new(
        "example::generic_helper",
        &language.catalog().expect("explicit standard providers"),
    );
    let identity = module
        .define_function(FunctionDecl::new("identity"))
        .unwrap();
    module
        .function(&identity, |function| {
            let value = function.type_parameter("T")?.ty();
            function.parameter("value", value.clone());
            function.returns(value);
            Ok(())
        })
        .unwrap();
    module
        .bind_with(
            identity,
            NativeBinding::new(vec![Codec::Value], Codec::Value, |cx| cx.argument(0)),
        )
        .unwrap();
    let singleton = module
        .define_function(FunctionDecl::new("singleton"))
        .unwrap();
    module
        .function(&singleton, |function| {
            let item = function.type_parameter("T")?.ty();
            function.parameter("value", item.clone());
            function.returns(language.vec(item));
            Ok(())
        })
        .unwrap();
    module
        .bind_with(
            singleton,
            NativeBinding::new(vec![Codec::Value], Codec::Value, |cx| {
                let result = cx.allocate_result()?;
                let root = cx.heap().root_value(result).unwrap();
                let Value::Array(id) = result else {
                    panic!("declared array result");
                };
                cx.heap().array_push(id, cx.argument(0)?)?;
                drop(root);
                Ok(result)
            }),
        )
        .unwrap();
    module.finish().unwrap()
}

#[test]
fn shared_native_returns_preserve_the_callers_layout_generation() {
    let module = generic_identity_module();
    let source = r#"
use example::generic_helper::identity;
use example::generic_helper::singleton;
trait Forward { fn forward<T>(self, value: T) -> T { singleton(identity(value))[0] } }
struct Source {}
impl Forward for Source {}
struct Item { val value: i32 }
fn receiver() -> Forward { Source {} }
trait Run { fn run(self, source: Forward) -> i32 { source.forward(Item { value: 42 }).value } }
impl Run for i32 {}
fn runner() -> Run { 7 }
"#;
    let mut runtime = Runtime::default();
    kagari_runtime::native::module::NativeModule::install_all(
        &kagari_stdlib::modules().unwrap(),
        &mut runtime,
    )
    .unwrap();
    module.install(&mut runtime).unwrap();
    let old = runtime
        .load_program("native-scope", compile_program(source, Some(&module)))
        .unwrap();
    let mut vm = Vm::new(runtime);
    let receiver = vm
        .execute(&old, "receiver")
        .unwrap()
        .return_value
        .value(vm.runtime().gc())
        .expect("retained execution result");
    let root = vm.runtime().root_value(receiver).unwrap();
    let next_source = source
        .replace("val value: i32", "val value: i32, val extra: bool")
        .replace("value: 42 }", "value: 42, extra: true }");
    let candidate = vm
        .runtime_mut()
        .stage_reload_program(
            &old,
            "native-scope",
            compile_program(&next_source, Some(&module)),
        )
        .unwrap();
    let current = vm.runtime_mut().publish_staged_reload(candidate).unwrap();
    let runner = vm
        .execute(&current, "runner")
        .unwrap()
        .return_value
        .value(vm.runtime().gc())
        .expect("retained execution result");
    let method = current
        .bytecode
        .interface_tables
        .iter()
        .flat_map(|table| &table.methods)
        .find(|method| {
            current
                .definitions()
                .resolve(method.method)
                .unwrap()
                .segments()
                .last()
                .unwrap()
                .name
                == "run"
        })
        .unwrap()
        .method;
    assert_eq!(
        vm.invoke_interface_method(&runner, &method, &[receiver])
            .unwrap(),
        Value::I32(42)
    );
    drop(root);
    assert!(
        vm.runtime()
            .collect_garbage()
            .unwrap()
            .reclaimed_modules
            .contains(&old.key())
    );
    assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
}

#[test]
fn generic_native_default_calls_the_selected_receiver_operation() {
    let mut module = ModuleBuilder::new(
        "example::receiver",
        &StandardDeclarations::default()
            .catalog()
            .expect("explicit standard providers"),
    );
    let mut declaration = module.define_trait("Read");
    let read = declaration
        .define_method(MethodDecl::instance("read").returns(Type::i32()))
        .unwrap();
    let echo = declaration
        .define_method(MethodDecl::instance("echo"))
        .unwrap();
    let operation = declaration.operation(&read).unwrap();
    let selected = declaration
        .method(&echo, |method| {
            let value = method.type_parameter("T")?.ty();
            method.parameter("value", value.clone());
            method.returns(value);
            Ok(method.requires(operation))
        })
        .unwrap();
    declaration
        .bind_default_with(
            echo,
            NativeBinding::new(vec![Codec::Value, Codec::Value], Codec::Value, move |cx| {
                let receiver = cx.argument(0)?;
                let target = cx.selected(&selected)?;
                if cx.call_values(target, &[receiver])? != Value::I32(42) {
                    return Err(RuntimeError::module_validation(
                        "selected receiver returned wrong value",
                    ));
                }
                cx.argument(1)
            }),
        )
        .unwrap();
    declaration.finish().unwrap();
    let module = module.finish().unwrap();
    let (vm, loaded) = compile(
        r#"
use example::receiver::Read;
struct Source<R> { val value: R }
impl<R> Read for Source<R> { fn read(self) -> i32 { 42 } }
trait Relay {
    fn echo<T>(self, value: T) -> T {
        val source: Read = Source { value };
        source.echo(value)
    }
}
impl Relay for i32 {}
struct Item { val value: i32 }
fn main() -> i32 {
    val source: Read = Source { value: 1 };
    val relay: Relay = 0;
    val result = relay.echo(Item { value: 42 });
    if source.echo("ok") == "ok" { source.echo(result.value) } else { 0 }
}
"#,
        Some(&module),
    );
    assert_eq!(
        vm.execute(&loaded, "main")
            .unwrap()
            .return_value
            .value(vm.runtime().gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
}

#[test]
fn a_shared_native_helper_calls_a_generic_constraint_member() {
    let mut module = ModuleBuilder::new(
        "example::generic_operation",
        &StandardDeclarations::default()
            .catalog()
            .expect("explicit standard providers"),
    );
    let mut declaration = module.define_trait("Identity");
    let method = declaration
        .define_method(MethodDecl::instance("identity"))
        .unwrap();
    declaration
        .method(&method, |method| {
            let value = method.type_parameter("T")?.ty();
            method.parameter("value", value.clone());
            method.returns(value);
            Ok(())
        })
        .unwrap();
    let contract = declaration.finish().unwrap();
    let invoke = module.define_function(FunctionDecl::new("invoke")).unwrap();
    let selected = module
        .function(&invoke, |function| {
            let source = function.type_parameter("S")?.ty();
            let value = function.type_parameter("T")?.ty();
            function.parameter("source", source.clone());
            function.parameter("value", value.clone());
            function.returns(value.clone());
            function.bound(source.clone(), contract.apply([]));
            Ok(function.requires(
                CallableRequirement::method(source, contract.method("identity")?)
                    .arguments([value]),
            ))
        })
        .unwrap();
    module
        .bind_with(
            invoke,
            NativeBinding::new(vec![Codec::Value, Codec::Value], Codec::Value, move |cx| {
                let arguments = [cx.argument(0)?, cx.argument(1)?];
                cx.call_values(cx.selected(&selected)?, &arguments)
            }),
        )
        .unwrap();
    let module = module.finish().unwrap();
    let (vm, loaded) = compile(
        r#"
use example::generic_operation::{Identity, invoke};
struct Source { var calls: i32 }
impl Identity for Source { fn identity<T>(self, value: T) -> T { self.calls += 1; value } }
trait Relay {
    fn relay<S: Identity, T>(self, source: S, value: T) -> T { invoke(source, value) }
}
impl Relay for i32 {}
struct Item { val value: i32 }
fn main() -> i32 {
    val relay: Relay = 0;
    val source = Source { calls: 0 };
    val result = relay.relay(source, Item { value: 42 });
    if relay.relay(source, "ok") == "ok" && source.calls == 2 { result.value } else { 0 }
}
"#,
        Some(&module),
    );
    assert_eq!(
        vm.execute(&loaded, "main")
            .unwrap()
            .return_value
            .value(vm.runtime().gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
}

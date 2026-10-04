use kagari_contract::callable::CallableImplementation;
use kagari_contract::library;
use {kagari_bytecode::program::ModuleRef, kagari_embed::context::JitPolicy};

use kagari_embed::{
    BytecodeArtifact,
    context::ExecutionContext,
    engine::{EngineConfig, KagariEngine},
    program::PreparedProgram,
};
use kagari_runtime::value::Value;
use kagari_source::source::SourceFile;

fn execute(source: &str) {
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let engine = KagariEngine::new(config);
    let artifact = engine
        .compile_to_artifact(SourceFile::new("defaults.kgr", source), Default::default())
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
        assert_eq!(result.return_value, Value::I32(42));
    }
}

#[test]
fn default_body_calls_the_implementation_override_through_static_and_dynamic_dispatch() {
    execute(
        r#"
trait Read { fn read(self) -> i32; fn twice(self) -> i32 { helper(self.read()) } }
fn helper(x: i32) -> i32 { x * 2 }
struct Number {}
impl Read for Number { fn read(self) -> i32 { 7 } }
fn generic<T: Read>(value: T) -> i32 { value.twice() }
fn main() -> i32 { val dynamic: Read = Number {}; Number {}.twice() + generic(Number {}) + dynamic.twice() }
"#,
    );
}

#[test]
fn explicit_override_takes_precedence_over_the_default_body() {
    execute(
        r#"
trait Read { fn read(self) -> i32 { 0 } }
struct Number {}
impl Read for Number { fn read(self) -> i32 { 21 } }
fn main() -> i32 { val dynamic: Read = Number {}; Number {}.read() + dynamic.read() }
"#,
    );
}

#[test]
fn generic_implementations_and_default_local_binders_are_specialized() {
    execute(
        r#"
trait Read<T> {
    fn read(self) -> T;
    fn again(self) -> T { self.read() }
}

struct Holder<T> { val value: T }
impl<T> Read<T> for Holder<T> { fn read(self) -> T { self.value } }
fn boxed<T>(x: Holder<T>) -> Read<T> { x }
fn main() -> i32 { boxed(Holder { value: 20 }).again() + Holder { value: 22 }.again() }
"#,
    );
    execute(
        r#"
trait Identity { fn identity<T>(self, value: T) -> T { value } fn same(self) -> Self { self } }
struct Number { val value: i32 }
impl Identity for Number {}
fn generic<T: Identity>(x: T) -> i32 { x.identity(42) }
fn main() -> i32 { val n = Number { value: 42 }.same(); generic(n) }
"#,
    );
}

#[test]
fn generic_default_method_is_callable_through_one_interface_with_distinct_arguments() {
    execute(
        r#"
trait Identity { fn identity<T>(self, value: T) -> T { value } }
struct Number {}
impl Identity for Number {}
fn main() -> i32 {
    val value: Identity = Number {};
    val text: String = value.identity("key");
    if text == "key" { value.identity(42) } else { 0 }
}
"#,
    );
}

#[test]
fn generic_interface_override_and_inherited_default_preserve_gc_values() {
    execute(
        r#"
trait Identity { fn identity<T>(self, value: T) -> T { value } }
trait Child: Identity {}
struct Number {}
struct Item { val value: i32 }
impl Identity for Number { fn identity<T>(self, value: T) -> T { value } }
impl Child for Number {}
fn main() -> i32 {
    val value: Child = Number {};
    val item: Item = value.identity(Item { value: 42 });
    item.value
}
"#,
    );
}

#[test]
fn generic_interface_default_forwards_its_binder_to_another_interface() {
    execute(
        r#"
trait Identity { fn identity<T>(self, value: T) -> T { value } }
trait Forward { fn forward<T>(self, target: Identity, value: T) -> T { target.identity(value) } }
struct Number {}
impl Identity for Number {}
impl Forward for Number {}
fn main() -> i32 {
    val target: Identity = Number {};
    val source: Forward = Number {};
    val text: String = source.forward(target, "key");
    if text == "key" { source.forward(target, 42) } else { 0 }
}
"#,
    );
}

#[test]
fn generic_interface_default_returns_a_closure_retaining_its_type_environment() {
    execute(
        r#"
trait Capture { fn capture<T>(self, value: T) -> fn() -> T { || value } }
struct Number {}
struct Item { val value: i32 }
impl Capture for Number {}
fn main() -> i32 {
    val source: Capture = Number {};
    val get = source.capture(Item { value: 42 });
    get().value
}
"#,
    );
}

#[test]
fn defaults_can_call_inherited_methods_and_use_associated_outputs() {
    execute(
        r#"
trait Read { type Item; fn read(self) -> Self::Item; fn again(self) -> Self::Item { self.read() } }
trait Child: Read<Item = i32> { fn number(self) -> i32 { self.again() } }
struct Number {}
impl Read for Number { type Item = i32; fn read(self) -> i32 { 21 } }
impl Child for Number {}
fn main() -> i32 { val child: Child = Number {}; child.number() + child.again() }
"#,
    );
}

#[test]
fn generic_interface_default_preserves_nested_result_types() {
    execute(
        r#"
trait Wrap { fn wrap<T>(self, value: T) -> Option<Option<T>> { Some(Some(value)) } }
struct Number {}
impl Wrap for Number {}
fn main() -> i32 {
    val source: Wrap = Number {};
    match source.wrap(42) { Some(inner) => match inner { Some(value) => value, None => 0 }, None => 0 }
}
"#,
    );
}

#[test]
fn generic_interface_keeps_trait_and_method_binders_distinct() {
    execute(
        r#"
trait Read<T> {
    fn read(self) -> T;
    fn pair<K>(self, key: K) -> (T, K) { (self.read(), key) }
}
struct Number {}
impl Read<i32> for Number { fn read(self) -> i32 { 42 } }
fn main() -> i32 {
    val source: Read<i32> = Number {};
    match source.pair("key") { (value, key) => if key == "key" { value } else { 0 } }
}
"#,
    );
}

#[test]
fn bounded_generic_interface_default_forwards_its_constraint() {
    execute(
        r#"
trait Identity { fn identity<T: Ord>(self, value: T) -> T { value } }
trait Forward { fn forward<T: Ord>(self, target: Identity, value: T) -> T { target.identity(value) } }
struct Number {}
impl Identity for Number {}
impl Forward for Number {}
fn main() -> i32 {
    val target: Identity = Number {};
    val source: Forward = Number {};
    source.forward(target, 42)
}
"#,
    );
}

#[test]
fn imported_defaults_preserve_private_helpers_and_definition_context() {
    use {
        kagari_common::identity::{ModuleIdentity, PackageId},
        kagari_source::source_database::SourceLayer,
    };
    let engine = KagariEngine::default();
    let mut root = None;
    for (name, source) in [
        (
            "model",
            "pub trait Read<T> { fn read(self) -> T; fn again(self) -> T { helper(self.read()) } } fn helper<T>(x: T) -> T { x }",
        ),
        (
            "root",
            "use pkg::model::Read; struct Holder<T> { val value: T } impl<T> Read<T> for Holder<T> { fn read(self) -> T { self.value } } fn helper(x: i32) -> i32 { 0 } fn boxed<T>(x: Holder<T>) -> Read<T> { x } fn main() -> i32 { boxed(Holder { value: 42 }).again() }",
        ),
    ] {
        let path = format!("mem://{name}");
        engine
            .bind_module(
                &path,
                ModuleIdentity {
                    package: PackageId("pkg".into()),
                    path: vec![name.into()],
                },
            )
            .unwrap();
        let id = engine
            .set_source(&path, source.into(), SourceLayer::Base)
            .unwrap();
        if name == "root" {
            root = Some(id);
        }
    }
    let checked = engine
        .compile_snapshot(engine.source_snapshot(), root.unwrap(), &Default::default())
        .unwrap();
    let artifact = engine.emit_bytecode(&checked, Default::default()).unwrap();
    let module = &artifact.program.modules[artifact.program.root.index()];
    let default = module
        .functions
        .iter()
        .find(|function| {
            function
                .identity
                .as_ref()
                .is_some_and(|identity| identity.declaration.path.last().unwrap().name == "again")
        })
        .unwrap();
    let origin = default.metadata.debug.source_module.unwrap();
    assert_eq!(
        artifact.program.modules[origin.index()].identity.path,
        ["model"]
    );
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime
        .load_program(
            &PreparedProgram::from_artifact(
                BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap(),
                &Default::default(),
                &Default::default(),
            )
            .unwrap(),
            Default::default(),
        )
        .unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, "main", &[], &context)
            .unwrap()
            .return_value,
        Value::I32(42)
    );
}

#[test]
fn invalid_default_bodies_are_checked_even_without_an_implementation() {
    let engine = KagariEngine::default();
    for source in [
        "trait Bad { fn bad(self) -> i32 { missing } } fn main() {}",
        "trait Bad { fn bad(self) -> i32 { true } } fn main() {}",
        "trait Bad { fn required(self) -> i32; } struct Number {} impl Bad for Number {} fn main() {}",
        "trait Bad { fn bad(self) -> Self { 0 } } fn main() {}",
    ] {
        assert!(
            engine
                .compile_to_artifact(
                    SourceFile::new("invalid-default.kgr", source),
                    Default::default()
                )
                .is_err(),
            "accepted {source}"
        );
    }
}

#[test]
fn default_closures_and_bound_helpers_substitute_the_concrete_self_receiver() {
    execute(
        r#"
trait Read {
    fn read(self) -> i32;
    fn again(self) -> i32 { val call = || helper(self); call() }
}
fn helper<T: Read>(value: T) -> i32 { value.read() }
struct Number {}
impl Read for Number { fn read(self) -> i32 { 42 } }
fn main() -> i32 { Number {}.again() }
"#,
    );
}

#[test]
fn malformed_default_contracts_and_source_origins_are_rejected() {
    let artifact = KagariEngine::default().compile_to_artifact(SourceFile::new("defaults.kgr", "pub trait Read { fn read(self) -> i32 { 42 } } struct Number {} impl Read for Number {} fn main() -> i32 { Number {}.read() }"),  Default::default()).unwrap();
    for mutation in 0..5 {
        let mut program = artifact.program.clone();
        let module = &mut program.modules[program.root.index()];
        if mutation < 3 {
            let contract = module
                .public_items
                .iter_mut()
                .find_map(|item| {
                    let kagari_contract::types::PublicItem::Trait(contract) = item else {
                        return None;
                    };
                    Some(contract)
                })
                .unwrap();
            if mutation == 0 {
                contract.methods.push(contract.methods[0].clone());
            } else if mutation == 1 {
                contract.methods[0].name = "missing_implementation".into();
            } else {
                contract.methods[0].implementation =
                    CallableImplementation::Native(library::trait_id("List"));
            }
        } else if mutation == 3 {
            let implementation = module
                .public_items
                .iter_mut()
                .find_map(|item| {
                    if let kagari_contract::types::PublicItem::InterfaceTable(table) = item {
                        Some(table)
                    } else {
                        None
                    }
                })
                .unwrap();
            implementation.methods.clear();
        } else {
            module.functions[0].metadata.debug.source_module = Some(ModuleRef::new(999));
        }
        assert!(BytecodeArtifact::from_program(program, Default::default()).is_err());
    }
}

#[test]
fn forged_shared_generic_environments_and_applications_are_rejected_without_source() {
    use kagari_bytecode::instruction::{BytecodeInstruction, CallTarget};
    use kagari_contract::{scalar::BuiltinType, types::Ty};
    let artifact = KagariEngine::default()
        .compile_to_artifact(
            SourceFile::new(
                "generics.kgr",
                r#"
trait Capture { fn capture<T>(self, value: T) -> fn() -> T { || value } }
struct Number {}
impl Capture for Number {}
fn main() -> i32 { val source: Capture = Number {}; val get = source.capture(42); get() }
"#,
            ),
            Default::default(),
        )
        .unwrap();
    for mutation in 0..5 {
        let mut program = artifact.program.clone();
        let module = &mut program.modules[program.root.index()];
        if mutation < 2 {
            let contract = module
                .functions
                .iter_mut()
                .flat_map(|function| &mut function.instructions)
                .find_map(|instruction| match instruction {
                    BytecodeInstruction::Call {
                        callee: CallTarget::InterfaceMethod { contract, .. },
                        ..
                    } => Some(contract),
                    _ => None,
                })
                .unwrap();
            if mutation == 0 {
                contract.arguments.clear();
            } else {
                contract.arguments[0] = Ty::Builtin(BuiltinType::String);
            }
        } else {
            let function = module
                .functions
                .iter_mut()
                .find(|function| function.metadata.semantic.generic.is_some())
                .unwrap();
            if mutation == 2 {
                function.metadata.semantic.generic = None;
            } else if mutation == 3 {
                function
                    .metadata
                    .semantic
                    .generic
                    .as_mut()
                    .unwrap()
                    .parameters[0]
                    .position += 1;
            } else {
                function.metadata.semantic.params.remove(&1);
            }
        }
        assert!(
            BytecodeArtifact::from_program(program, Default::default()).is_err(),
            "mutation {mutation}"
        );
    }
}

#[test]
fn shared_generic_method_constructs_a_typed_list_result() {
    execute(
        r#"use std::collections::{List};

trait Wrap { fn wrap<T>(self, value: T) -> List<T> { [value] } }
struct Source {}
impl Wrap for Source {}
struct Item { val value: i32 }
fn main() -> i32 {
    val source: Wrap = Source {};
    val a = source.wrap(21);
    val b = source.wrap(Item { value: 21 });
    val nested = source.wrap(b);
    var sum = 0;
    for item in a { sum += item; }
    for item in nested[0] { sum += item.value; }
    if sum != 42 { return 0; }
    a[0] + b[0].value
}
"#,
    );
}

#[test]
fn shared_list_table_mappings_are_checked_without_source() {
    use kagari_bytecode::instruction::BytecodeInstruction;
    use kagari_contract::{scalar::BuiltinType, types::Ty};
    let artifact = KagariEngine::default()
        .compile_to_artifact(
            SourceFile::new(
                "shared-list.kgr",
                r#"use std::collections::{List};

trait Wrap { fn wrap<T>(self, value: T) -> List<T> { [value] } }
struct Source {}
impl Wrap for Source {}
fn main() -> i32 { val source: Wrap = Source {}; source.wrap(42)[0] }
"#,
            ),
            Default::default(),
        )
        .unwrap();
    for mutation in 0..6 {
        let mut program = artifact.program.clone();
        if mutation == 0 {
            let arguments = program
                .modules
                .iter_mut()
                .flat_map(|module| &mut module.functions)
                .flat_map(|function| &mut function.instructions)
                .find_map(|instruction| match instruction {
                    BytecodeInstruction::MakeInterface { arguments, .. }
                        if arguments.iter().any(|ty| !ty.is_concrete()) =>
                    {
                        Some(arguments)
                    }
                    _ => None,
                })
                .unwrap();
            arguments[0] = Ty::Builtin(BuiltinType::String);
        } else if mutation == 5 {
            let view = program
                .modules
                .iter_mut()
                .flat_map(|module| &mut module.interface_tables)
                .filter(|table| table.arguments.iter().any(|ty| !ty.is_concrete()))
                .find_map(|table| table.view.as_mut())
                .unwrap();
            view.results[0].implementation.arguments.clear();
        } else {
            let table = program
                .modules
                .iter_mut()
                .flat_map(|module| &mut module.interface_tables)
                .find(|table| {
                    !table.parents.is_empty() && table.arguments.iter().any(|ty| !ty.is_concrete())
                })
                .unwrap();
            match mutation {
                1 => table.methods[0].arguments.clear(),
                2 => table.parents.clear(),
                3 => {
                    table.parents[0].implementation.arguments[0] = Ty::Builtin(BuiltinType::String)
                }
                4 => {
                    let Ty::Parameter { position, .. } = &mut table.arguments[0] else {
                        panic!("shared table binder")
                    };
                    *position += 1;
                }
                _ => unreachable!(),
            }
        }
        assert!(
            BytecodeArtifact::from_program(program, Default::default()).is_err(),
            "mutation {mutation}"
        );
    }
}

#[test]
fn shared_generic_method_calls_an_ordinary_generic_helper() {
    execute(
        r#"
fn identity<T>(value: T) -> T { value }
trait Forward { fn forward<T>(self, value: T) -> T { identity(value) } }
struct Source {}
impl Forward for Source {}
struct Item { val value: i32 }
fn main() -> i32 {
    val source: Forward = Source {};
    source.forward(21) + source.forward(Item { value: 21 }).value
}
"#,
    );
}

#[test]
fn shared_helpers_forward_bounds_and_retain_nested_types() {
    execute(
        r#"
fn invoke<F: Fn() -> i32>(callback: F) -> i32 { callback() }
fn retain<T>(value: T, remaining: i32) -> fn() -> T {
    if remaining == 0 { || value } else { retain(value, remaining - 1) }
}
trait Run {
    fn run<F: Fn() -> i32>(self, callback: F) -> i32 { invoke(callback) }
    fn keep<T>(self, value: T) -> fn() -> (T, i32) { retain((value, 1), 2) }
}
struct Source {}
impl Run for Source {}
struct Callback { val value: i32 }
impl Fn<()> for Callback { type Output = i32; fn call(self, args: ()) -> i32 { self.value } }
fn main() -> i32 {
    val source: Run = Source {};
    val get = source.keep(Callback { value: 20 });
    val pair = get();
    source.run(pair[0]) + source.run(|| 21) + pair[1]
}
"#,
    );
}

#[test]
fn shared_function_call_arguments_and_operations_are_verified_without_source() {
    use kagari_bytecode::instruction::{BytecodeInstruction, CallTarget};
    use kagari_contract::{scalar::BuiltinType, types::Ty};
    let artifact = KagariEngine::default()
        .compile_to_artifact(
            SourceFile::new(
                "shared-helper.kgr",
                r#"
fn invoke<F: Fn() -> i32>(callback: F) -> i32 { callback() }
trait Run { fn run<F: Fn() -> i32>(self, callback: F) -> i32 { invoke(callback) } }
struct Source {}
impl Run for Source {}
fn main() -> i32 { val source: Run = Source {}; source.run(|| 42) }
"#,
            ),
            Default::default(),
        )
        .unwrap();
    for mutation in 0..6 {
        let mut program = artifact.program.clone();
        let call = program
            .modules
            .iter_mut()
            .flat_map(|module| &mut module.functions)
            .flat_map(|function| &mut function.instructions)
            .find_map(|instruction| match instruction {
                BytecodeInstruction::Call {
                    callee: CallTarget::Shared { contract, .. },
                    ..
                } => Some(contract),
                _ => None,
            })
            .unwrap();
        match mutation {
            0 => call.arguments.clear(),
            1 => call.signature.params[0] = Ty::Builtin(BuiltinType::I32),
            2 => call.operations.clear(),
            3 => call.operations.push(call.operations[0].clone()),
            4 => call.instance.declaration.path.last_mut().unwrap().name = "forged".into(),
            5 => {
                let Ty::Parameter { position, .. } = &mut call.arguments[0] else {
                    panic!("caller binder")
                };
                *position += 1;
            }
            _ => unreachable!(),
        }
        assert!(
            BytecodeArtifact::from_program(program, Default::default()).is_err(),
            "mutation {mutation}"
        );
    }
}

#[test]
fn shared_helpers_are_materialized_in_their_declaring_module() {
    use {
        kagari_common::identity::{ModuleIdentity, PackageId},
        kagari_source::source_database::SourceLayer,
    };
    let engine = KagariEngine::default();
    let mut root = None;
    for (name, source) in [
        (
            "model",
            "fn retain<T>(value: T) -> fn() -> T { || value } pub trait Keep { fn keep<T>(self, value: T) -> fn() -> T { retain(value) } }",
        ),
        (
            "root",
            "use pkg::model::Keep; struct Source {} impl Keep for Source {} struct Item { val value: i32 } fn main() -> i32 { val source: Keep = Source {}; val get = source.keep(Item { value: 42 }); get().value }",
        ),
    ] {
        let path = format!("mem://{name}");
        engine
            .bind_module(
                &path,
                ModuleIdentity {
                    package: PackageId("pkg".into()),
                    path: vec![name.into()],
                },
            )
            .unwrap();
        let revision = engine
            .set_source(&path, source.into(), SourceLayer::Base)
            .unwrap();
        if name == "root" {
            root = Some(revision);
        }
    }
    let checked = engine
        .compile_snapshot(engine.source_snapshot(), root.unwrap(), &Default::default())
        .unwrap();
    let artifact = engine.emit_bytecode(&checked, Default::default()).unwrap();
    let decoded = BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    let program =
        PreparedProgram::from_artifact(decoded, &Default::default(), &Default::default()).unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, "main", &[], &context)
            .unwrap()
            .return_value,
        Value::I32(42)
    );
}

#[test]
fn shared_generic_methods_construct_nominal_values() {
    execute(
        r#"
struct Box<T> { var value: T }
trait Wrap {
    fn wrap<T>(self, value: T) -> Box<T> { Box { value: value } }
    fn update<T>(self, target: Box<T>, value: T) -> T {
        val old = target.value;
        target.value = value;
        old
    }
}
struct Source {}
impl Wrap for Source {}
struct Item { val value: i32 }
fn main() -> i32 {
    val source: Wrap = Source {};
    val left = source.wrap(20);
    val right = source.wrap(Item { value: 21 });
    if source.update(left, 21) != 20 { return 0; }
    val previous = source.update(right, Item { value: 21 });
    if previous.value != 21 { return 0; }
    left.value + right.value.value
}
"#,
    );
}

#[test]
fn shared_methods_upcast_generic_interfaces() {
    execute(
        r#"use std::collections::{List, MutableList};

trait Wrap {
    fn wrap<T>(self, value: T) -> List<T> {
        val mutable: MutableList<T> = [value];
        val readonly: List<T> = mutable;
        readonly
    }
}
struct Source {}
impl Wrap for Source {}
struct Item { val value: i32 }
fn main() -> i32 {
    val source: Wrap = Source {};
    source.wrap(21)[0] + source.wrap(Item { value: 21 })[0].value
}
"#,
    );
}

#[test]
fn generic_interface_upcasts_reject_forged_scopes_and_parents() {
    use kagari_bytecode::instruction::BytecodeInstruction;
    use kagari_contract::{scalar::BuiltinType, types::Ty};
    let artifact = KagariEngine::default()
        .compile_to_artifact(
            SourceFile::new(
                "shared-upcast.kgr",
                r#"use std::collections::{List, MutableList};

trait Wrap {
    fn wrap<T>(self, value: T) -> List<T> {
        val mutable: MutableList<T> = [value];
        mutable
    }
}
struct Source {}
impl Wrap for Source {}
fn main() -> i32 { val source: Wrap = Source {}; source.wrap(42)[0] }
"#,
            ),
            Default::default(),
        )
        .unwrap();
    for mutation in 0..4 {
        let mut program = artifact.program.clone();
        let (source, target) = program
            .modules
            .iter_mut()
            .flat_map(|module| &mut module.functions)
            .flat_map(|function| &mut function.instructions)
            .find_map(|instruction| match instruction {
                BytecodeInstruction::UpcastInterface { source, target, .. } => {
                    Some((source, target))
                }
                _ => None,
            })
            .unwrap();
        match mutation {
            0 => {
                let Ty::Parameter { position, .. } = &mut target.arguments[0] else {
                    panic!("shared binder")
                };
                *position += 1;
            }
            1 => target.arguments[0] = Ty::Builtin(BuiltinType::String),
            2 => target.arguments.clear(),
            3 => std::mem::swap(source, target),
            _ => unreachable!(),
        }
        assert!(
            BytecodeArtifact::from_program(program, Default::default()).is_err(),
            "mutation {mutation}"
        );
    }
}

#[test]
fn shared_interface_tables_supply_receiver_bound_operations() {
    execute(
        r#"use std::cmp::{Ordering};

trait Reader<T: Ord> {
    fn lesser(self, left: T, right: T) -> T { if left < right { left } else { right } }
}
struct Source {}
impl<T: Ord> Reader<T> for Source {}
trait Choose {
    fn reader<T: Ord>(self, value: T) -> Reader<T> { Source {} }
    fn choose<T: Ord>(self, left: T, right: T) -> T {
        val reader: Reader<T> = Source {};
        reader.lesser(left, right)
    }
}
impl Choose for Source {}
struct Rank { val value: i32 }
impl PartialEq for Rank { fn eq(self, other: Self) -> bool { self.value == other.value } }
impl Eq for Rank {}
impl PartialOrd for Rank { fn partial_cmp(self, other: Self) -> Option<Ordering> { self.value.partial_cmp(other.value) } }
impl Ord for Rank { fn cmp(self, other: Self) -> Ordering { self.value.cmp(other.value) } }
fn main() -> i32 {
    val chooser: Choose = Source {};
    val scalar = chooser.choose(20, 43);
    val reader = chooser.reader(Rank { value: 22 });
    val concrete: Reader<i32> = Source {};
    if concrete.lesser(1, 2) != 1 { return 0; }
    scalar + reader.lesser(Rank { value: 22 }, Rank { value: 23 }).value
}
"#,
    );
}

#[test]
fn ordinary_interface_methods_require_receiver_bound_witnesses() {
    use kagari_bytecode::instruction::{BytecodeInstruction, CallTarget};
    let artifact = KagariEngine::default()
        .compile_to_artifact(
            SourceFile::new(
                "receiver-bound.kgr",
                r#"
trait Reader<T: Ord> {
    fn lesser(self, left: T, right: T) -> T { if left < right { left } else { right } }
}
struct Source {}
impl<T: Ord> Reader<T> for Source {}
trait Choose {
    fn choose<T: Ord>(self, left: T, right: T) -> T {
        val reader: Reader<T> = Source {};
        reader.lesser(left, right)
    }
}
impl Choose for Source {}
fn main() -> i32 { val chooser: Choose = Source {}; chooser.choose(42, 43) }
"#,
            ),
            Default::default(),
        )
        .unwrap();
    let mut program = artifact.program;
    let call = program
        .modules
        .iter_mut()
        .flat_map(|module| &mut module.functions)
        .flat_map(|function| &mut function.instructions)
        .find_map(|instruction| match instruction {
            BytecodeInstruction::Call {
                callee: CallTarget::InterfaceMethod { contract, .. },
                ..
            } if contract.arguments.is_empty() && !contract.operations.is_empty() => Some(contract),
            _ => None,
        })
        .unwrap();
    call.operations.clear();
    assert!(BytecodeArtifact::from_program(program, Default::default()).is_err());
}

#[test]
fn shared_nominal_layouts_preserve_nested_fields_and_enum_patterns() {
    execute(
        r#"use std::collections::{List};

struct Box<T> { var value: T }
enum Wrapped<T> { Some(T), None }
trait Wrap {
    fn wrap<T>(self, value: T) -> List<Box<Wrapped<T>>> {
        [Box { value: Wrapped::Some(value) }]
    }
    fn unwrap<T>(self, value: Box<Wrapped<T>>, fallback: T) -> T {
        match value.value { Wrapped::Some(inner) => inner, Wrapped::None => fallback }
    }
}
struct Source {}
impl Wrap for Source {}
struct Item { val value: i32 }
fn main() -> i32 {
    val source: Wrap = Source {};
    val scalar = source.wrap(20);
    val object = source.wrap(Item { value: 22 });
    source.unwrap(scalar[0], 0) + source.unwrap(object[0], Item { value: 0 }).value
}
"#,
    );
}

#[test]
fn shared_nominal_layout_applications_are_verified_without_source() {
    use kagari_bytecode::instruction::BytecodeInstruction;
    use kagari_contract::{scalar::BuiltinType, types::Ty};
    let artifact = KagariEngine::default()
        .compile_to_artifact(
            SourceFile::new(
                "generic-layout.kgr",
                r#"
struct Box<T> { var value: T }
trait Wrap {
    fn wrap<T>(self, value: T) -> Box<T> { Box { value: value } }
    fn read<T>(self, value: Box<T>) -> T { value.value }
}
struct Source {}
impl Wrap for Source {}
fn main() -> i32 { val source: Wrap = Source {}; source.read(source.wrap(42)) }
"#,
            ),
            Default::default(),
        )
        .unwrap();
    for mutation in 0..6 {
        let mut program = artifact.program.clone();
        if mutation < 2 {
            let layout = program
                .modules
                .iter_mut()
                .flat_map(|module| &mut module.structures)
                .find(|layout| layout.arguments.iter().any(|ty| !ty.is_concrete()))
                .unwrap();
            let ty = if mutation == 0 {
                &mut layout.arguments[0]
            } else {
                &mut layout.fields[0].ty
            };
            let Ty::Parameter { position, .. } = ty else {
                panic!("layout binder")
            };
            *position += 1;
        } else if mutation == 5 {
            let layout = program
                .modules
                .iter_mut()
                .flat_map(|module| &mut module.structures)
                .find(|layout| {
                    !layout.arguments.is_empty() && layout.arguments.iter().all(Ty::is_concrete)
                })
                .unwrap();
            layout.fields[0].ty = Ty::Builtin(BuiltinType::String);
        } else {
            let arguments = program
                .modules
                .iter_mut()
                .flat_map(|module| &mut module.functions)
                .flat_map(|function| &mut function.instructions)
                .find_map(|instruction| match instruction {
                    BytecodeInstruction::MakeStruct { arguments, .. }
                        if mutation != 4 && arguments.iter().any(|ty| !ty.is_concrete()) =>
                    {
                        Some(arguments)
                    }
                    BytecodeInstruction::ReadAggregateField { field, .. }
                        if mutation == 4 && field.arguments.iter().any(|ty| !ty.is_concrete()) =>
                    {
                        Some(&mut field.arguments)
                    }
                    _ => None,
                })
                .unwrap();
            if mutation == 2 {
                arguments.clear();
            } else if mutation == 3 {
                let Ty::Parameter { position, .. } = &mut arguments[0] else {
                    panic!("caller binder")
                };
                *position += 1;
            } else {
                arguments[0] = Ty::Builtin(BuiltinType::String);
            }
        }
        assert!(
            BytecodeArtifact::from_program(program, Default::default()).is_err(),
            "mutation {mutation}"
        );
    }
}

#[test]
fn shared_constraint_calls_a_generic_member_and_preserves_its_override() {
    execute(
        r#"
trait Identity { fn identity<T>(self, value: T) -> T { value } }
struct Source { var calls: i32 }
impl Identity for Source {
    fn identity<T>(self, value: T) -> T { self.calls += 1; value }
}
fn forward<S: Identity, T>(source: S, value: T) -> T { source.identity(value) }
trait Relay {
    fn relay<S: Identity, T>(self, source: S, value: T) -> T {
        forward(source, value)
    }
}
impl Relay for i32 {}
struct Item { val value: i32 }
fn main() -> i32 {
    val relay: Relay = 0;
    val source = Source { calls: 0 };
    val result = relay.relay(source, Item { value: 42 });
    val text: String = relay.relay(source, "checked");
    if source.calls == 2 && text == "checked" { result.value } else { 0 }
}
"#,
    );
}

#[test]
fn forged_shared_constraint_method_selections_are_rejected_without_source() {
    use kagari_bytecode::instruction::{BytecodeInstruction, CallTarget};
    use kagari_contract::callable::witness::OperationWitness;
    let artifact = KagariEngine::default()
        .compile_to_artifact(
            SourceFile::new(
                "generic-witness.kgr",
                r#"
trait Identity { fn identity<T>(self, value: T) -> T { value } }
impl Identity for i32 {}
trait Relay { fn relay<S: Identity>(self, source: S) -> i32 { source.identity(42) } }
impl Relay for i32 {}
fn main() -> i32 { val relay: Relay = 0; relay.relay(0) }
"#,
            ),
            Default::default(),
        )
        .unwrap();
    for mutation in 0..4 {
        let mut program = artifact.program.clone();
        let selected = program
            .modules
            .iter_mut()
            .flat_map(|module| &mut module.functions)
            .flat_map(|function| &mut function.instructions)
            .find_map(|instruction| {
                let BytecodeInstruction::Call {
                    callee: CallTarget::InterfaceMethod { contract, .. },
                    ..
                } = instruction
                else {
                    return None;
                };
                contract
                    .operations
                    .iter_mut()
                    .find_map(|operation| match operation {
                        OperationWitness::SharedMethod(selected) => Some(selected),
                        _ => None,
                    })
            })
            .unwrap();
        let identity = selected.implementation.clone();
        match mutation {
            0 => selected.requirement.member.path.last_mut().unwrap().name = "missing".into(),
            1 => {
                selected
                    .implementation
                    .declaration
                    .path
                    .last_mut()
                    .unwrap()
                    .occurrence += 1
            }
            2 => selected
                .implementation
                .arguments
                .push(selected.requirement.receiver.clone()),
            _ => {
                for module in &mut program.modules {
                    module.interface_tables.retain(|table| {
                        table.declaration != identity.declaration
                            || table.arguments != identity.arguments
                    });
                }
            }
        }
        assert!(
            BytecodeArtifact::from_program(program, Default::default()).is_err(),
            "forged shared method {mutation}"
        );
    }
}

#[test]
fn generic_constraint_members_receive_their_local_bound_operations() {
    execute(
        r#"use std::cmp::{Ordering};

trait Compare<R> { fn choose<K: Ord>(self, left: K, right: K) -> K; }
struct Source<R> { val value: R }
impl<R> Compare<R> for Source<R> {
    fn choose<K: Ord>(self, left: K, right: K) -> K {
        if left.cmp(right) == Ordering::Greater { left } else { right }
    }
}
trait Relay {
    fn relay<S: Compare<i32>, K: Ord>(self, source: S, left: K, right: K) -> K {
        source.choose(left, right)
    }
}
impl Relay for i32 {}
fn main() -> i32 {
    val relay: Relay = 0;
    val source = Source { value: 1 };
    val text: String = relay.relay(source, "a", "b");
    if text == "b" { relay.relay(source, 21, 42) } else { 0 }
}
"#,
    );
}

use super::compile_program;
use crate::Runtime;
#[cfg(test)]
use crate::test_support::bytecode::load_bytecode;
use crate::{
    RuntimeConfig,
    error::RuntimeErrorKind,
    host::{HostCallContext, HostObjectId},
    metadata::TypeId,
    module::ExecutableModule,
    native::{
        builder::ModuleBuilder, declarations::FunctionDecl, module::NativeModule, types::Type,
    },
    value::Value,
};
use kagari_bytecode::{module::BytecodeModule, program::ModuleRef};
use kagari_common::identity::{DefinitionKind, map::DefinitionContext};
use kagari_types::{
    declaration::host::{
        HostCallPolicy, HostFunctionEffects, HostPassingStyle, HostReflectionPolicy,
        HostTypeOwnership, HostTypePolicy, PathAccess,
    },
    language::binding,
    scalar::BuiltinType,
    ty::Ty,
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

// Reuse the runtime contract owners without another invocation or ABI implementation.
use crate::test_support::native::invoke as native_invocation;
use crate::test_support::native::{program as native_program, program::ProgramFixture};

fn host_policy(parameters: usize) -> HostCallPolicy {
    HostCallPolicy {
        passing: vec![HostPassingStyle::Owned; parameters],
        effects: HostFunctionEffects {
            may_allocate: true,
            ..Default::default()
        },
    }
}

fn native_slot(
    runtime: &Runtime,
    owner: &ExecutableModule,
    provider: &NativeModule,
    name: &str,
) -> usize {
    let query = super::support::declaration_query(provider, DefinitionKind::Function, name);
    let declaration = runtime.host().definitions().lookup(&query).unwrap();
    owner
        .bytecode
        .native_imports
        .iter()
        .position(|import| import.instance.declaration == declaration)
        .unwrap()
}

fn runtime() -> Runtime {
    let mut runtime = Runtime::new(RuntimeConfig::default());
    NativeModule::install_all(&kagari_stdlib::modules().unwrap(), &mut runtime).unwrap();
    runtime
}

fn allocation_owner(runtime: &mut Runtime) -> ExecutableModule {
    let program = load_bytecode(
        runtime,
        "allocation-owner",
        ProgramFixture {
            exports: Default::default(),
            root: ModuleRef::new(0),
            modules: vec![BytecodeModule::default()],
        }
        .metadata(
            DefinitionContext::new().unwrap().snapshot(),
            &Default::default(),
        )
        .unwrap(),
    )
    .unwrap();
    runtime.select_program(&program).unwrap()
}

#[test]
fn nested_arguments_and_results_obey_the_complete_host_signature() {
    let mut runtime = runtime();
    let fixture_source = "pub fn fixtures() -> ([i32], [bool]) { ([7], [true]) }";
    let program = compile_program(fixture_source, None);
    let loaded = load_bytecode(&mut runtime, "host-composites", program).unwrap();
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let mut vm = runtime;
    let Value::Tuple(values) = vm
        .execute_module(&loaded, "fixtures", vm.execution_options())
        .unwrap()
        .return_value
        .value(vm.gc())
        .expect("retained execution result")
    else {
        panic!("fixtures")
    };
    let values = vm.gc().tuple(values).unwrap().to_vec();
    let [array, wrong_array] = values.as_slice() else {
        panic!("two fixtures")
    };
    let runtime = &mut vm;
    let definitions = runtime.host().definitions();
    let enumeration = |ty, member, fields| {
        let applied = runtime
            .resolve_type_arguments(&loaded, &[ty])
            .unwrap()
            .remove(0);
        runtime
            .make_enum_member(&loaded, &applied, member, fields)
            .unwrap()
    };
    let some = |value| {
        let ty = match value {
            Value::Bool(_) => BuiltinType::Bool,
            _ => BuiltinType::I32,
        };
        enumeration(
            binding::option(&definitions, Ty::Builtin(ty)).unwrap(),
            "Some",
            vec![value],
        )
    };
    let result = |member, value| {
        let error = if member == "Ok" || matches!(value, Value::Str(_)) {
            BuiltinType::String
        } else {
            BuiltinType::I32
        };
        enumeration(
            binding::result(
                &definitions,
                Ty::Builtin(BuiltinType::I32),
                Ty::Builtin(error),
            )
            .unwrap(),
            member,
            vec![value],
        )
    };
    let cases = [
        (
            Ty::Tuple(vec![
                Ty::Array(Box::new(Ty::Builtin(BuiltinType::I32))),
                Ty::Builtin(BuiltinType::String),
            ]),
            runtime
                .gc()
                .alloc_tuple(vec![
                    *array,
                    runtime.gc().alloc_string("ok".into()).unwrap(),
                ])
                .unwrap(),
            runtime
                .gc()
                .alloc_tuple(vec![
                    *wrong_array,
                    runtime.gc().alloc_string("ok".into()).unwrap(),
                ])
                .unwrap(),
        ),
        (
            Ty::Array(Box::new(Ty::Builtin(BuiltinType::I32))),
            *array,
            *wrong_array,
        ),
        (
            binding::option(&definitions, Ty::Builtin(BuiltinType::I32)).unwrap(),
            some(Value::I32(7)),
            some(Value::Bool(true)),
        ),
        (
            binding::option(&definitions, Ty::Builtin(BuiltinType::I32)).unwrap(),
            enumeration(
                binding::option(&definitions, Ty::Builtin(BuiltinType::I32)).unwrap(),
                "None",
                vec![],
            ),
            result("Ok", Value::I32(7)),
        ),
        (
            binding::option(&definitions, Ty::Builtin(BuiltinType::I32)).unwrap(),
            enumeration(
                binding::option(&definitions, Ty::Builtin(BuiltinType::I32)).unwrap(),
                "None",
                vec![],
            ),
            enumeration(
                binding::option(&definitions, Ty::Builtin(BuiltinType::Bool)).unwrap(),
                "None",
                vec![],
            ),
        ),
        (
            binding::result(
                &definitions,
                Ty::Builtin(BuiltinType::I32),
                Ty::Builtin(BuiltinType::String),
            )
            .unwrap(),
            result("Ok", Value::I32(7)),
            result("Err", Value::I32(7)),
        ),
        (
            binding::result(
                &definitions,
                Ty::Builtin(BuiltinType::I32),
                Ty::Builtin(BuiltinType::String),
            )
            .unwrap(),
            result("Err", runtime.gc().alloc_string("error".into()).unwrap()),
            some(Value::I32(7)),
        ),
    ];
    let catalog = kagari_stdlib::declarations::StandardDeclarations::default()
        .catalog()
        .unwrap();
    let mut builder = ModuleBuilder::new("fixtures::composites", &catalog).unwrap();
    let mut source = format!("{fixture_source}\nuse fixtures::composites;\n");
    let syntax = [
        "([i32], String)",
        "[i32]",
        "Option<i32>",
        "Option<i32>",
        "Option<i32>",
        "Result<i32,String>",
        "Result<i32,String>",
    ];
    let mut checks = Vec::new();
    for (index, (ty, good, bad)) in cases.into_iter().enumerate() {
        let ty = Type::from_semantic(ty, &definitions).unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let called = calls.clone();
        let echo = builder
            .define_function(
                FunctionDecl::new(format!("echo{index}"))
                    .parameter("value", ty.clone())
                    .returns(ty.clone())
                    .host_policy(host_policy(1)),
            )
            .unwrap();
        builder
            .bind(echo, move |_: &HostCallContext<'_>, args: &[Value]| {
                called.fetch_add(1, Ordering::SeqCst);
                Ok(args[0])
            })
            .unwrap();
        let called = calls.clone();
        let output = builder
            .define_function(
                FunctionDecl::new(format!("bad{index}"))
                    .returns(ty)
                    .host_policy(host_policy(0)),
            )
            .unwrap();
        builder
            .bind(output, move |_: &HostCallContext<'_>, _: &[Value]| {
                called.fetch_add(1, Ordering::SeqCst);
                Ok(bad)
            })
            .unwrap();
        let ty = syntax[index];
        source.push_str(&format!(
            "fn echo{index}(value: {ty}) -> {ty} {{ composites::echo{index}(value) }} fn bad{index}() -> {ty} {{ composites::bad{index}() }}\n"
        ));
        checks.push((good, bad, calls));
    }
    let provider = builder.finish().unwrap();
    let program = compile_program(&source, Some(&provider));
    provider.install(runtime).unwrap();
    // Values and host signatures share an installation, including nominal
    // collection/enum/interface evidence retained across this additive reload.
    let candidate = runtime
        .stage_reload_program(&_loaded_program, None, program)
        .unwrap();
    runtime.publish_staged_reload(candidate).unwrap();
    let owner = runtime.select_program(&_loaded_program).unwrap();
    for (index, (good, bad, calls)) in checks.into_iter().enumerate() {
        let echo = native_slot(runtime, &owner, &provider, &format!("echo{index}"));
        let output = native_slot(runtime, &owner, &provider, &format!("bad{index}"));
        assert_eq!(
            native_invocation::invoke(runtime, &owner, echo, &[good])
                .unwrap_or_else(|error| panic!("case {index}: {error:?}")),
            good
        );
        assert!(native_invocation::invoke(runtime, &owner, echo, &[bad]).is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(native_invocation::invoke(runtime, &owner, output, &[]).is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }
}

#[test]
fn composite_arguments_are_rooted_during_callbacks_and_reject_foreign_or_stale_handles() {
    let mut runtime = runtime();
    let owner = allocation_owner(&mut runtime);
    let calls = Arc::new(AtomicUsize::new(0));
    let called = calls.clone();
    let mut builder = ModuleBuilder::new("fixtures::rooted", &Default::default()).unwrap();
    let ty = Type::tuple([Type::from_semantic(
        Ty::Array(Box::new(Ty::Builtin(BuiltinType::I32))),
        &DefinitionContext::new().unwrap().snapshot(),
    )
    .unwrap()])
    .unwrap();
    let echo = builder
        .define_function(
            FunctionDecl::new("echo")
                .parameter("value", ty.clone())
                .returns(ty)
                .host_policy(host_policy(1)),
        )
        .unwrap();
    builder
        .bind(
            echo,
            move |context: &HostCallContext<'_>, args: &[Value]| {
                called.fetch_add(1, Ordering::SeqCst);
                context.runtime().collect_garbage().unwrap();
                Ok(args[0])
            },
        )
        .unwrap();
    let provider = builder.finish().unwrap();
    let program = native_program::free_function_program(&provider.to_declaration());
    provider.install(&mut runtime).unwrap();
    let entry = load_bytecode(&mut runtime, "rooted-composite", program).unwrap();
    let _entry_program = entry;
    let entry = runtime.select_program(&_entry_program).unwrap();
    let id = native_slot(&runtime, &entry, &provider, "echo");
    let array = runtime
        .alloc_array(&owner, Ty::Builtin(BuiltinType::I32), vec![Value::I32(7)])
        .unwrap();
    let value = runtime.gc().alloc_tuple(vec![Value::Array(array)]).unwrap();
    assert_eq!(
        native_invocation::invoke(&runtime, &entry, id, std::slice::from_ref(&value)).unwrap(),
        value
    );
    assert!(runtime.gc().array_snapshot(array).is_some());
    let mut other = Runtime::default();
    NativeModule::install_all(&kagari_stdlib::modules().unwrap(), &mut other).unwrap();
    let other_owner = allocation_owner(&mut other);
    let foreign = other
        .gc()
        .alloc_tuple(vec![Value::Array(
            other
                .alloc_array(
                    &other_owner,
                    Ty::Builtin(BuiltinType::I32),
                    vec![Value::I32(7)],
                )
                .unwrap(),
        )])
        .unwrap();
    assert!(native_invocation::invoke(&runtime, &entry, id, &[foreign]).is_err());
    runtime.collect_garbage().unwrap();
    assert!(runtime.gc().array_snapshot(array).is_none());
    assert!(native_invocation::invoke(&runtime, &entry, id, &[value]).is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn owned_composites_cannot_hide_frame_scoped_host_borrows() {
    let mut runtime = runtime();
    let owner = allocation_owner(&mut runtime);
    let mut builder = ModuleBuilder::new("fixtures::escape", &Default::default()).unwrap();
    let mut object = builder.define_type("Object").unwrap();
    object
        .external(HostTypePolicy {
            ownership: HostTypeOwnership::HostRoot,
            reflection: HostReflectionPolicy::Hidden,
            path_access: PathAccess::None,
        })
        .unwrap();
    let object = object.finish().unwrap();
    let consume = builder
        .define_function(
            FunctionDecl::new("consume")
                .parameter(
                    "value",
                    Type::tuple([Type::tuple([object.apply([]).unwrap()]).unwrap()]).unwrap(),
                )
                .host_policy(host_policy(1)),
        )
        .unwrap();
    builder
        .bind(consume, |_: &HostCallContext<'_>, _: &[Value]| {
            panic!("an owned parameter must reject nested borrows before the callback")
        })
        .unwrap();
    let provider = builder.finish().unwrap();
    let program = native_program::free_function_program(&provider.to_declaration());
    provider.install(&mut runtime).unwrap();
    let entry = load_bytecode(&mut runtime, "owned-composite", program).unwrap();
    let _entry_program = entry;
    let entry = runtime.select_program(&_entry_program).unwrap();
    let id = native_slot(&runtime, &entry, &provider, "consume");
    for unique in [false, true] {
        let scope = runtime.host_scope(&[]).unwrap();
        let value = if unique {
            runtime
                .gc()
                .alloc_host_mut(
                    scope
                        .borrows()
                        .borrow_unique(HostObjectId(1), TypeId::new(0))
                        .unwrap(),
                )
                .unwrap()
        } else {
            runtime
                .gc()
                .alloc_host_ref(
                    scope
                        .borrows()
                        .borrow_shared(HostObjectId(1), TypeId::new(0))
                        .unwrap(),
                )
                .unwrap()
        };
        let value = runtime
            .gc()
            .alloc_tuple(vec![runtime.gc().alloc_tuple(vec![value]).unwrap()])
            .unwrap();
        assert_eq!(
            native_invocation::invoke(&runtime, &entry, id, std::slice::from_ref(&value))
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::HostBorrowEscape
        );
        assert!(
            runtime
                .alloc_array(&owner, Ty::Builtin(BuiltinType::I32), vec![value])
                .is_err()
        );
        drop(scope);
        assert!(native_invocation::invoke(&runtime, &entry, id, &[value]).is_err());
    }
    assert!(!runtime.is_quarantined());
}

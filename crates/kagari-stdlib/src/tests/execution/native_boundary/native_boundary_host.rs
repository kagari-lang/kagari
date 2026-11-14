use super::compile_program;
#[cfg(test)]
use crate::tests::support::bytecode::load_bytecode;
use kagari_runtime::{
    Runtime, RuntimeConfig,
    error::RuntimeErrorKind,
    host::HostError,
    module::ExecutableModule,
    native::{
        builder::ModuleBuilder, conversion::context::ConversionContext, declarations::FunctionDecl,
        host, module::NativeModule, types::Type,
    },
    value_semantics::{identity_equal, script_equal},
};

use kagari_common::identity::{DefinitionKind, map::DefinitionContext};
use kagari_types::{
    declaration::host::{HostCallPolicy, HostFunctionEffects, HostPassingStyle},
    language::binding,
    scalar::BuiltinType,
    ty::Ty,
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

// Reuse the runtime contract owners without another invocation or ABI implementation.
use crate::tests::support::native::{invoke as native_invocation, program as native_program};

fn host_policy(parameters: usize) -> HostCallPolicy {
    HostCallPolicy {
        passing: vec![HostPassingStyle::Owned; parameters],
        effects: HostFunctionEffects {
            may_allocate: true,
            ..Default::default()
        },
    }
}

fn native_slot(owner: &ExecutableModule, provider: &NativeModule, name: &str) -> usize {
    let query = super::support::declaration_query(provider, DefinitionKind::Function, name);
    let declaration = owner.definitions().lookup(&query).unwrap();
    owner
        .bytecode
        .native_imports
        .iter()
        .position(|import| import.instance.declaration == declaration)
        .unwrap()
}

fn runtime() -> Runtime {
    let mut runtime = Runtime::new(RuntimeConfig::default());
    NativeModule::install_all(&crate::modules().unwrap(), &mut runtime).unwrap();
    runtime
}

fn value_owner(runtime: &mut Runtime) -> ExecutableModule {
    let program = load_bytecode(
        runtime,
        "value-owner",
        compile_program(
            include_str!("../../../../tests/fixtures/native/host-rooted-argument.kgr"),
            None,
        ),
    )
    .unwrap();
    runtime.select_program(&program).unwrap()
}

#[test]
fn nested_arguments_and_results_obey_the_complete_host_signature() {
    let fixture_source =
        include_str!("../../../../tests/fixtures/native/host-composite-values.kgr");
    let mut runtime = runtime();
    let installed = load_bytecode(
        &mut runtime,
        "host-composites",
        compile_program(fixture_source, None),
    )
    .unwrap();
    let loaded = runtime.select_program(&installed).unwrap();
    let definitions = runtime.host().definitions();
    let option = binding::option(&definitions, Ty::Builtin(BuiltinType::I32)).unwrap();
    let result = binding::result(
        &definitions,
        Ty::Builtin(BuiltinType::I32),
        Ty::Builtin(BuiltinType::String),
    )
    .unwrap();
    let cases = [
        (
            "tuple_values",
            "([i32], String)",
            Ty::Tuple(vec![
                Ty::Array(Box::new(Ty::Builtin(BuiltinType::I32))),
                Ty::Builtin(BuiltinType::String),
            ]),
        ),
        (
            "array_values",
            "[i32]",
            Ty::Array(Box::new(Ty::Builtin(BuiltinType::I32))),
        ),
        ("option_values", "Option<i32>", option.clone()),
        ("option_result", "Option<i32>", option.clone()),
        ("option_empty", "Option<i32>", option),
        ("result_error_type", "Result<i32, String>", result.clone()),
        ("result_option", "Result<i32, String>", result),
    ];
    let catalog = crate::declarations::StandardDeclarations::default()
        .catalog()
        .unwrap();
    let mut builder = ModuleBuilder::new("fixtures::composites", &catalog).unwrap();
    let mut source = format!("{fixture_source}\nuse fixtures::composites;\n");
    let mut checks = Vec::new();
    for (index, (entry, syntax, ty)) in cases.into_iter().enumerate() {
        let report = runtime
            .execute_module(&loaded, entry, runtime.execution_options())
            .unwrap();
        let (good, bad) = {
            let mut cx = ConversionContext::new(&mut runtime, &loaded).unwrap();
            let tuple = cx.import(&report.return_value).unwrap();
            assert_eq!(cx.tuple_len(&tuple).unwrap(), 2);
            let good = cx.tuple_element(&tuple, 0).unwrap();
            let bad = cx.tuple_element(&tuple, 1).unwrap();
            (cx.retain(&good).unwrap(), cx.retain(&bad).unwrap())
        };
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
            .bind(
                echo,
                host::callback(move |_, args| {
                    called.fetch_add(1, Ordering::SeqCst);
                    Ok(args[0])
                }),
            )
            .unwrap();
        let called = calls.clone();
        let output = builder
            .define_function(
                FunctionDecl::new(format!("bad{index}"))
                    .returns(ty)
                    .host_policy(host_policy(0)),
            )
            .unwrap();
        let bad_result = bad.clone();
        builder
            .bind(
                output,
                host::callback(move |cx, _| {
                    called.fetch_add(1, Ordering::SeqCst);
                    cx.import(&bad_result)
                        .map_err(|error| HostError::new(error.to_string()))
                }),
            )
            .unwrap();
        // Only the closed declaration/wrapper arity varies. Test programs that
        // manufacture the checked composite values live in the fixed fixture.
        source.push_str(&format!(
            "fn echo{index}(value: {syntax}) -> {syntax} {{ composites::echo{index}(value) }} fn bad{index}() -> {syntax} {{ composites::bad{index}() }}\n"
        ));
        checks.push((good, bad, calls));
    }
    let provider = builder.finish().unwrap();
    let program = compile_program(&source, Some(&provider));
    provider.install(&mut runtime).unwrap();
    // The additive reload preserves installation/nominal identity for values
    // constructed before the external Host declarations were installed.
    let candidate = runtime
        .stage_reload_program(&installed, None, program)
        .unwrap();
    runtime.publish_staged_reload(candidate).unwrap();
    let owner = runtime.select_program(&installed).unwrap();
    for (index, (good, bad, calls)) in checks.into_iter().enumerate() {
        runtime.collect_garbage().unwrap();
        let roots = runtime.gc_stats().root_registrations;
        let echo = native_slot(&owner, &provider, &format!("echo{index}"));
        let output = native_slot(&owner, &provider, &format!("bad{index}"));
        let returned =
            native_invocation::invoke(&mut runtime, &owner, echo, std::slice::from_ref(&good))
                .unwrap_or_else(|error| panic!("case {index}: {error:?}"));
        if index == 0 {
            let mut cx = ConversionContext::new(&mut runtime, &owner).unwrap();
            let good_view = cx.import(&good).unwrap();
            let returned_view = cx.import(&returned).unwrap();
            let original_array = cx.tuple_element(&good_view, 0).unwrap();
            let returned_array = cx.tuple_element(&returned_view, 0).unwrap();
            let original_array = cx.retain(&original_array).unwrap();
            let returned_array = cx.retain(&returned_array).unwrap();
            assert!(identity_equal(cx.runtime_mut(), &original_array, &returned_array).unwrap());
            let text = cx.tuple_element(&returned_view, 1).unwrap();
            let expected = cx.type_for::<String>().unwrap();
            assert_eq!(cx.decode_value::<String>(&expected, &text).unwrap(), "ok");
        } else if index == 1 {
            assert!(identity_equal(&mut runtime, &good, &returned).unwrap());
        } else {
            assert!(script_equal(&mut runtime, &good, &returned).unwrap());
        }
        drop(returned);
        assert!(native_invocation::invoke(&mut runtime, &owner, echo, &[bad]).is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(native_invocation::invoke(&mut runtime, &owner, output, &[]).is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        runtime.collect_garbage().unwrap();
        assert_eq!(runtime.gc_stats().root_registrations, roots);
    }
}

#[test]
fn composite_arguments_survive_callback_collection_and_reject_foreign_ownership() {
    let mut runtime = runtime();
    let owner = value_owner(&mut runtime);
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
            host::callback(move |cx, args| {
                called.fetch_add(1, Ordering::SeqCst);
                cx.runtime_mut().collect_garbage().unwrap();
                Ok(args[0])
            }),
        )
        .unwrap();
    let provider = builder.finish().unwrap();
    let program = native_program::free_function_program(&provider.to_declaration());
    provider.install(&mut runtime).unwrap();
    let installed = load_bytecode(&mut runtime, "rooted-composite", program).unwrap();
    let entry = runtime.select_program(&installed).unwrap();
    let id = native_slot(&entry, &provider, "echo");
    runtime.collect_garbage().unwrap();
    let baseline = runtime.gc_stats();
    let value = runtime
        .execute_module(&owner, "make", runtime.execution_options())
        .unwrap()
        .return_value;
    let returned =
        native_invocation::invoke(&mut runtime, &entry, id, std::slice::from_ref(&value)).unwrap();
    drop(value);
    runtime.collect_garbage().unwrap();
    {
        let mut cx = ConversionContext::new(&mut runtime, &owner).unwrap();
        let tuple = cx.import(&returned).unwrap();
        assert_eq!(cx.tuple_len(&tuple).unwrap(), 1);
        let array = cx.tuple_element(&tuple, 0).unwrap();
        assert_eq!(cx.array_len(&array).unwrap(), 1);
        assert_eq!(
            cx.array_element(&array, 0usize).unwrap().unwrap().as_i32(),
            Some(7)
        );
    }
    let mut other = Runtime::default();
    NativeModule::install_all(&crate::modules().unwrap(), &mut other).unwrap();
    let other_owner = value_owner(&mut other);
    let foreign = other
        .execute_module(&other_owner, "make", other.execution_options())
        .unwrap()
        .return_value;
    assert_eq!(
        native_invocation::invoke(&mut runtime, &entry, id, std::slice::from_ref(&foreign))
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ModuleValidation
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    // Genuine roots cannot become stale merely because collection runs. Dropping
    // the last owning result instead establishes actual graph reclamation.
    drop(returned);
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, baseline.objects);
    assert_eq!(
        runtime.gc_stats().root_registrations,
        baseline.root_registrations
    );
    assert!(!runtime.is_quarantined());
}

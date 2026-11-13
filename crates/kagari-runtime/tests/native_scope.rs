//! Actual Native scopes keep one state value and retire per-step scratch.
use crate::{bytecode_fixture::load_bytecode, internal_calls::string_provider};
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_hir::{analysis::AnalysisDatabase, compile_input::program::CheckedCompilation};
use kagari_runtime::{
    Runtime,
    context::ExecutionContext,
    error::{RuntimeError, RuntimeErrorKind},
    frame::{
        ExecutionStack,
        cursor::kernel::RegionExit,
        driver::{ExecutionAction, ReturnAction},
    },
    native::{
        binding::{Codec, NativeBinding, NativeResult},
        builder::ModuleBuilder,
        catalog::DeclarationCatalog,
        conversion::context::ConversionContext,
        declarations::FunctionDecl,
        scalar::NativeScalar,
        types::Type,
    },
    value::carriers::RootedValue,
};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_types::scalar::BuiltinType;
use std::{
    ops::ControlFlow,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::Arc,
};

#[test]
fn native_scratch_replaces_managed_state_and_retires_on_all_exits() {
    let cancelled = ExecutionContext::default();
    let cancellation = cancelled.cancellation.clone();
    let strings = string_provider();
    let catalog = DeclarationCatalog::from_modules(&[&strings]).unwrap();
    let mut builder = ModuleBuilder::new("example::scratch", &catalog).unwrap();
    let string = Type::scalar(BuiltinType::String);
    let reduce = builder
        .define_function(
            FunctionDecl::new("reduce")
                .parameter("value", string.clone())
                .parameter("count", Type::u64())
                .returns(string),
        )
        .unwrap();
    builder
        .bind(
            reduce,
            NativeBinding::scoped(vec![Codec::Value; 2], Codec::Value, move |cx| {
                let initial = cx.argument(0)?;
                let count = u64::decode(cx.argument(1)?)?;
                assert!(count > 0);
                let element = cx.argument_type_argument(0)?;
                let array = cx.allocate_array(element, &[initial])?;
                assert_eq!(cx.array_len(&array)?, 1);
                assert!(cx.array_element(&array, u128::MAX)?.is_none());
                let child = cx
                    .array_element(&array, 0usize)?
                    .expect("present array child");
                assert!(cx.read_strings(&[initial, child], |[left, right]| left == right)?);
                let error: NativeResult<()> = cx.with_temporaries(|temporary| {
                    temporary.allocate_string("failed temporary".into())?;
                    Err(RuntimeError::host_call_failure("deliberate scope failure"))
                });
                assert_eq!(error.unwrap_err().kind(), RuntimeErrorKind::HostCallFailure);
                assert!(cx.collect_garbage()?.retired_owners >= 1);
                let unwind = catch_unwind(AssertUnwindSafe(|| {
                    cx.with_temporaries::<()>(|temporary| {
                        temporary.allocate_string("unwound temporary".into())?;
                        panic!("deliberate scope unwind");
                    })
                }));
                assert!(unwind.is_err());
                assert!(cx.collect_garbage()?.retired_owners >= 1);
                let absent = cx.with_optional_value_temporaries(|temporary| {
                    temporary.allocate_string("discarded temporary".into())?;
                    Ok(None)
                })?;
                assert!(absent.is_none());
                assert!(cx.collect_garbage()?.retired_owners >= 1);
                let mut round = 0;
                let result = cx.with_value_loop(&initial, |temporary, value| {
                    let collected = temporary.collect_garbage()?;
                    if round != 0 {
                        // Both discarded payloads from the preceding step must be dead.
                        // Keeping every callback result until Native return fails here.
                        assert!(collected.retired_owners >= 2);
                    }
                    let next = temporary.read_strings(&[value, value], |[left, right]| {
                        assert_eq!(left, right);
                        format!("{left}x")
                    })?;
                    temporary.allocate_string("first discarded payload".into())?;
                    temporary.allocate_string("second discarded payload".into())?;
                    let next = temporary.allocate_string(next)?;
                    round += 1;
                    if count == 64 && round == 3 {
                        cancellation.cancel();
                    }
                    Ok(if round == count {
                        ControlFlow::Break(next)
                    } else {
                        ControlFlow::Continue(next)
                    })
                })?;
                cx.collect_garbage()?;
                assert_eq!(cx.read_strings(&[initial], |[text]| text.to_owned())?, "a");
                // Staging through a short lend updates the original pending cell. Its
                // owner must survive that lend's retirement and later collection.
                cx.with_temporaries(|temporary| temporary.set_result(result))?;
                cx.collect_garbage()?;
                Ok(())
            }),
        )
        .unwrap();
    let provider = builder.finish().unwrap();
    let mut sources = SourceDatabase::default();
    let root = sources
        .set(
            "native_scope.kgr",
            include_str!("fixtures/native_scope.kgr").into(),
            SourceLayer::Base,
        )
        .unwrap();
    let mut analysis = AnalysisDatabase::default();
    analysis.set_native_modules(vec![
        Arc::new(strings.to_declaration()),
        Arc::new(provider.to_declaration()),
    ]);
    let snapshot = analysis
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let checked = snapshot.check_program(root, &Default::default()).unwrap();
    let mir = lower_program_to_mir(
        &CheckedCompilation::from_program(&checked, &Default::default()).unwrap(),
        &Default::default(),
    )
    .unwrap();
    let bytecode = lower_program_to_bytecode(&mir, &Default::default())
        .unwrap()
        .into_unverified();
    let mut runtime = Runtime::default();
    strings.install(&mut runtime).unwrap();
    provider.install(&mut runtime).unwrap();
    let program = load_bytecode(&mut runtime, "native-scope", bytecode.clone()).unwrap();
    let function = runtime
        .bind_function::<(String, u64), String>(&program.root(), "run")
        .unwrap();
    runtime.collect_garbage().unwrap();
    let baseline = runtime.gc_stats();
    // The public frame entry accepts source-bound views from Rust. A valid view
    // supplied by a different Runtime must reject without poisoning this one.
    let mut foreign = Runtime::default();
    strings.install(&mut foreign).unwrap();
    provider.install(&mut foreign).unwrap();
    let foreign_program = load_bytecode(&mut foreign, "foreign-source", bytecode).unwrap();
    let foreign_owner = foreign.select_program(&foreign_program).unwrap();
    let mut source = ConversionContext::new(&mut foreign, &foreign_owner).unwrap();
    let string = source.type_for::<String>().unwrap();
    let foreign_view = source.encode_value(&string, "foreign".to_owned()).unwrap();
    let owner = runtime.select_program(&program).unwrap();
    let entry = owner
        .bytecode
        .functions
        .iter()
        .find(|body| body.name == "run")
        .unwrap()
        .id;
    let mut activation = runtime.enter_execution_stack(&owner).unwrap();
    activation
        .with(|runtime, stack| {
            let arguments = [foreign_view, 1u64.encode()];
            let error = stack
                .push(runtime, owner.slot(), entry, &arguments, None)
                .unwrap_err();
            assert_eq!(error.kind(), RuntimeErrorKind::ModuleValidation);
            assert!(stack.is_empty(runtime)?);
            assert!(!runtime.is_quarantined());
            assert_eq!(runtime.resources().counters().current_call_depth, 0);
            Ok(())
        })
        .unwrap();
    drop(activation);
    // Two actual owned roots share the incoming string after its Host lease
    // retires. Parking one and retiring the other must keep the survivor intact.
    let arguments = {
        let mut source = ConversionContext::new(&mut runtime, &owner).unwrap();
        let string = source.type_for::<String>().unwrap();
        let value = source.encode_value(&string, "a".to_owned()).unwrap();
        [source.retain(&value).unwrap(), RootedValue::from(1u64)]
    };
    let first = runtime
        .start_owned_execution(&owner, entry, &arguments, Default::default())
        .unwrap();
    let second = runtime
        .start_owned_execution(&owner, entry, &arguments, Default::default())
        .unwrap();
    drop(arguments);
    for execution in [&first, &second] {
        let mut active = Some(runtime.resume_owned_execution(execution).unwrap());
        assert!(ExecutionStack::try_park_ready(&mut active).unwrap());
        assert!(active.is_none());
        assert!(execution.is_ready());
    }
    runtime.collect_garbage().unwrap();
    assert_eq!(
        runtime.gc_stats().root_registrations,
        baseline.root_registrations
    );
    runtime.finish_owned_execution(&first).unwrap();
    runtime.collect_garbage().unwrap();
    assert!(second.is_ready());
    let mut active = runtime.resume_owned_execution(&second).unwrap();
    let output = active
        .with(|runtime, stack| {
            assert!(matches!(
                stack
                    .execute_region(runtime, &mut None, |_, _, _, _, _| unreachable!())
                    .unwrap(),
                RegionExit::Call(ExecutionAction::NativeEntry)
            ));
            assert_eq!(
                stack.start_native_entry(runtime, |_, _, _, _, _| unreachable!())?,
                ExecutionAction::NativeReturn
            );
            assert!(matches!(
                stack.finish_native_return(runtime)?,
                ReturnAction::Continue(ExecutionAction::Script)
            ));
            let RegionExit::Return(value) = stack
                .execute_region(runtime, &mut None, |_, _, _, _, _| unreachable!())
                .unwrap()
            else {
                panic!("the resumed scratch caller must return its string");
            };
            let ReturnAction::Complete(value) = stack.finish_return(runtime, value)? else {
                panic!("the owned caller is the root frame");
            };
            stack.retain_result(runtime, value)
        })
        .unwrap();
    drop(active);
    runtime.finish_owned_execution(&second).unwrap();
    assert_eq!(
        ConversionContext::new(&mut runtime, &owner)
            .unwrap()
            .decode::<String>(&output)
            .unwrap(),
        "ax"
    );
    drop(output);
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, baseline.objects);
    assert_eq!(
        runtime.gc_stats().root_registrations,
        baseline.root_registrations
    );
    for count in [1, 32, 64, 1] {
        if count == 64 {
            let error = runtime
                .call_with_context(&function, ("a".into(), count), &cancelled)
                .unwrap_err();
            assert_eq!(error.kind(), RuntimeErrorKind::Cancelled);
        } else {
            assert_eq!(
                runtime.call(&function, ("a".into(), count)).unwrap(),
                format!("a{}", "x".repeat(count as usize))
            );
        }
        runtime.collect_garbage().unwrap();
        let current = runtime.gc_stats();
        assert_eq!(current.objects, baseline.objects);
        assert_eq!(current.root_registrations, baseline.root_registrations);
        assert_eq!(runtime.resources().counters().current_call_depth, 0);
        assert!(runtime.execution_id().is_none());
    }
}

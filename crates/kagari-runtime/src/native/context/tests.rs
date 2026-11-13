#[cfg(test)]
use crate::test_support::bytecode::load_bytecode;
use crate::{
    Runtime,
    error::RuntimeErrorKind,
    execution_metadata::{applications::ApplicationRecord, managed::sources::MetadataSource},
    frame::{FrameStack, driver::ReturnAction},
    module::{ExecutableModule, ModuleEpochRetention, managed::source::RevisionSource},
    native::{
        binding::{Codec, NativeBinding, NativeResult},
        builder::ModuleBuilder,
        catalog::DeclarationCatalog,
        context::{
            CallableOwner, LinkedCallable, LinkedOperation, NativeCall, NativeEntry, ScriptCall,
        },
        conversion::{
            arguments::ArgumentValues,
            context::{ConversionContext, native::NativeContract},
        },
        declarations::FunctionDecl,
        storage::NativeStorage,
        stored_selection::StoredSelection,
        types::Type,
    },
    test_support::native::{invoke::invoke, program::free_function_program},
    value::{
        carriers::RootedValue,
        kind::ObjectKind,
        place::{PublishedResult, ResultPlace},
        view::ValueView,
    },
};
use kagari_bytecode::{instruction::NativeImportId, module::CallableTarget};
use kagari_types::{scalar::BuiltinType, ty::Ty};
use std::slice;
use std::sync::{
    Arc, Mutex, OnceLock,
    atomic::{AtomicUsize, Ordering},
};

fn invoke_stored<'result>(
    runtime: &mut Runtime,
    owner: &ExecutableModule,
    target: ScriptCall<'_, '_>,
    arguments: ArgumentValues<'_>,
    destination: ResultPlace<'result>,
) -> NativeResult<PublishedResult<'result>> {
    // Stored ordinary callbacks select the current publication while keeping the
    // enclosing root session and its candidate/installation admission intact.
    assert_eq!(runtime.execution_root().unwrap().key(), owner.key());
    let ScriptCall::Selected(selected) = target else {
        panic!("stored selection requires a sealed source-backed invocation")
    };
    let mut stack_activation = runtime.enter_selected_execution_stack(selected)?;
    stack_activation.with(|runtime, stack| {
        stack.push_selected_call(runtime, selected, arguments)?;
        stack.start_native_entry(runtime, invoke_stored)?;
        let ReturnAction::Complete(value) = stack.finish_native_return(runtime)? else {
            panic!("stored native return")
        };
        stack.with_result(runtime, value, |runtime, value| {
            destination.publish(runtime, &value)
        })
    })
}

fn call_scalar_equal(
    runtime: &mut Runtime,
    owner: &ExecutableModule,
    target: &LinkedCallable,
    origin: RevisionSource<'_>,
) -> NativeResult<bool> {
    let CallableTarget::Native(import) = target.target else {
        unreachable!()
    };
    let function = runtime.modules.native_binding(owner, import).unwrap();
    let arguments: [ValueView<'static>; 2] = [ValueView::from(7i32), ValueView::from(7i32)];
    let mut context = NativeCall::new(
        runtime,
        owner,
        NativeEntry::Closed(&function),
        origin,
        ArgumentValues::scalars(&arguments),
        |_, _, _, _, _| panic!("scalar native callback must use direct entry"),
    )?;
    context.call(target, (7_i32, 7_i32))
}

crate::native_data! { struct Payload {} }

fn payload_with_selections(
    runtime: &mut Runtime,
    owner: &ExecutableModule,
    ty: Ty,
    selected: &[LinkedOperation],
) -> RootedValue {
    let applied = runtime
        .resolve_type_arguments(owner, &[ty])
        .unwrap()
        .pop()
        .unwrap();
    let Ty::NativeObject(nominal) = applied.ty() else {
        panic!("payload type")
    };
    let storage = runtime
        .native_entries
        .storage
        .get_id(nominal.declaration)
        .unwrap()
        .clone();
    let mut conversion = ConversionContext::new(runtime, owner).unwrap();
    let execution = conversion.execution_id().unwrap();
    let selections = selected
        .iter()
        .map(|operation| StoredSelection::new(owner, operation.ready().unwrap()))
        .collect::<NativeResult<Vec<_>>>()
        .unwrap();
    let source = MetadataSource::Revision {
        owner,
        source: conversion.owner_origin(),
    };
    let application = conversion
        .runtime_mut()
        .alloc_application(
            execution,
            ApplicationRecord::StoredSelections(selections.into()),
            &[source],
        )
        .unwrap();
    let value = conversion
        .allocate_native_data(
            NativeContract {
                storage: &storage,
                owner,
                applied: &applied,
            },
            Payload {},
            Some(&application),
        )
        .unwrap();
    conversion.retain(&value).unwrap()
}

fn push_probe(
    stack: &FrameStack,
    runtime: &mut Runtime,
    owner: &ExecutableModule,
    import: NativeImportId,
    value: &RootedValue,
) -> NativeResult<()> {
    stack.push_rooted_callable(
        runtime,
        owner.clone(),
        CallableTarget::Native(import),
        slice::from_ref(value),
    )
}

#[test]
fn stored_selections_select_current_with_frame_roots_and_checked_calls() {
    let mut builder = ModuleBuilder::new("test::stored", &DeclarationCatalog::default()).unwrap();
    let mut payload = builder.define_type("Payload").unwrap();
    payload
        .native_storage(NativeStorage::data::<Payload>())
        .unwrap();
    let payload = payload.finish().unwrap();
    let staged = builder
        .define_function(FunctionDecl::new("staged").returns(payload.apply([]).unwrap()))
        .unwrap();
    builder
        .bind(
            staged,
            NativeBinding::scoped(vec![], Codec::Value, |call| {
                let result = call.allocate_result_payload(Payload {}, &[])?;
                call.set_result(result)?;
                assert_eq!(
                    call.set_result(ValueView::unit()).unwrap_err().kind(),
                    RuntimeErrorKind::ModuleValidation
                );
                call.collect_garbage()?;
                assert_eq!(
                    call.validate_value(result)?.kind(),
                    Some(ObjectKind::Native),
                    "pending result survives collection before callback completion"
                );
                Ok(())
            }),
        )
        .unwrap();
    let probe = builder
        .define_function(
            FunctionDecl::new("probe")
                .parameter("payload", payload.apply([]).unwrap())
                .returns(Type::i64()),
        )
        .unwrap();
    builder
        .bind(
            probe,
            NativeBinding::new(vec![Codec::Value], Codec::Value, |cx| {
                cx.collect_garbage()?;
                cx.with_payload::<Payload, _>(0, |_| Ok(()))?;
                assert_eq!(
                    cx.call_stored_values(0, 1, &[ValueView::from(7i32), ValueView::from(7i32)])?
                        .as_bool(),
                    Some(true)
                );
                for (slot, args) in [
                    (2, vec![ValueView::from(7i32)]),
                    (0, vec![]),
                    (0, vec![ValueView::from(true)]),
                ] {
                    assert_eq!(
                        cx.call_stored_values(0, slot, &args).unwrap_err().kind(),
                        RuntimeErrorKind::ModuleValidation
                    );
                }
                let result = cx.call_stored_values(0, 0, &[ValueView::from(7i32)])?;
                cx.with_payload::<Payload, _>(0, |_| Ok(()))?;
                Ok(result)
            }),
        )
        .unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let hash = builder
        .define_function(
            FunctionDecl::new("hash")
                .parameter("key", Type::i32())
                .returns(Type::i64()),
        )
        .unwrap();
    builder
        .bind(
            hash,
            NativeBinding::new(vec![Codec::Value], Codec::Value, move |cx| {
                cx.collect_garbage()?;
                observed.fetch_add(1, Ordering::SeqCst);
                Ok(ValueView::from(11i64))
            }),
        )
        .unwrap();
    let equal = builder
        .define_function(
            FunctionDecl::new("equal")
                .parameter("left", Type::i32())
                .parameter("right", Type::i32())
                .returns(Type::bool()),
        )
        .unwrap();
    let expected_publication = Arc::new(Mutex::new(None));
    let expected_equal = expected_publication.clone();
    builder
        .bind(
            equal,
            NativeBinding::new(vec![Codec::Value, Codec::Value], Codec::Value, move |cx| {
                assert_eq!(
                    Some(cx.owner().publication_token()),
                    *expected_equal.lock().unwrap()
                );
                cx.collect_garbage()?;
                let left = cx.argument(0)?.as_i32().unwrap();
                let right = cx.argument(1)?.as_i32().unwrap();
                Ok(ValueView::from(left == right))
            }),
        )
        .unwrap();
    let native = builder.finish().unwrap();
    let code = free_function_program(&native.to_declaration());
    let mut runtime = Runtime::default();
    native.install(&mut runtime).unwrap();
    let old_program = load_bytecode(&mut runtime, "stored", code.clone()).unwrap();
    let old = runtime.select_program(&old_program).unwrap();
    let old_native_body = runtime
        .retain_program(
            &old,
            ModuleEpochRetention::RuntimeValue,
            RevisionSource::Current,
        )
        .unwrap();
    let import = |name: &str| {
        NativeImportId::new(
            old.bytecode
                .native_declarations
                .iter()
                .position(|decl| decl.function.name == name)
                .unwrap(),
        )
    };
    let probe = import("probe");
    let staged_import = import("staged");
    let selected = [
        (
            "hash",
            vec![Ty::Builtin(BuiltinType::I32)],
            Ty::Builtin(BuiltinType::I64),
        ),
        (
            "equal",
            vec![Ty::Builtin(BuiltinType::I32); 2],
            Ty::Builtin(BuiltinType::Bool),
        ),
    ]
    .map(|(name, params, result)| {
        LinkedOperation::Ready(LinkedCallable {
            environment: None,
            scoped_signature: OnceLock::new(),
            owner: CallableOwner::Program(old.slot()),
            target: CallableTarget::Native(import(name)),
            params: params.into(),
            result,
            primitive: None,
        })
    });
    let ty = old.bytecode.native_imports[probe.index()].signature.params[0].clone();
    let value = payload_with_selections(&mut runtime, &old, ty, &selected);
    let staged = runtime
        .stage_reload_program(&old_program, None, code.clone())
        .unwrap();
    runtime.publish_staged_reload(staged).unwrap();

    let current = runtime.select_program(&old_program).unwrap();
    runtime.collect_garbage().unwrap();
    let before_staged = runtime.gc_stats();
    let staged_result = invoke(&mut runtime, &current, staged_import.index(), &[]).unwrap();
    assert_eq!(
        runtime.gc_stats().root_registrations,
        before_staged.root_registrations + 1,
        "only the explicit returned Host handle remains"
    );
    runtime.collect_garbage().unwrap();
    let mut source = runtime.host_scope(&[]).unwrap();
    assert_eq!(
        source.import(&staged_result).unwrap().kind(),
        Some(ObjectKind::Native)
    );
    drop(source);
    drop(staged_result);
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, before_staged.objects);
    assert_eq!(
        runtime.gc_stats().root_registrations,
        before_staged.root_registrations
    );
    *expected_publication.lock().unwrap() = Some(current.publication_token());
    {
        let mut _stack_activation = runtime.enter_execution_stack(&current).unwrap();
        _stack_activation
            .with(|runtime, _stack| {
                assert!(
                    call_scalar_equal(
                        runtime,
                        &old,
                        selected[1].ready().unwrap(),
                        RevisionSource::Host(old_native_body.revision())
                    )
                    .unwrap()
                );

                Ok(())
            })
            .unwrap();
    }
    // A candidate must not use a stored edge to enter a published generation.
    let candidate = runtime
        .stage_reload_program(&old_program, None, code.clone())
        .unwrap();
    {
        let mut _session = runtime.begin_candidate_initialization(&candidate).unwrap();
        assert_eq!(
            call_scalar_equal(
                _session.runtime_mut(),
                &old,
                selected[1].ready().unwrap(),
                RevisionSource::Host(old_native_body.revision())
            )
            .unwrap_err()
            .kind(),
            RuntimeErrorKind::ExecutionPhaseViolation
        );
        let mut stack_activation = _session
            .runtime_mut()
            .enter_execution_stack(candidate.module())
            .unwrap();
        stack_activation
            .with(|runtime, stack| {
                assert_eq!(
                    push_probe(stack, runtime, candidate.module(), probe, &value)
                        .unwrap_err()
                        .kind(),
                    RuntimeErrorKind::ExecutionPhaseViolation
                );

                Ok(())
            })
            .unwrap();
    }
    drop(candidate);
    drop(old_native_body);
    runtime.collect_garbage().unwrap();
    let before_probe = runtime.gc_stats();
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    let mut foreign = Runtime::default();
    native.install(&mut foreign).unwrap();
    let foreign_owner_program = load_bytecode(&mut foreign, "stored", code).unwrap();
    let foreign_owner = foreign.select_program(&foreign_owner_program).unwrap();
    {
        let mut stack_activation = foreign.enter_execution_stack(&foreign_owner).unwrap();
        stack_activation
            .with(|foreign, stack| {
                assert!(push_probe(stack, foreign, &foreign_owner, probe, &value).is_err());

                Ok(())
            })
            .unwrap();
    }
    // Even a backend that returns a value of the wrong type cannot publish it.
    {
        let mut stack_activation = runtime.enter_execution_stack(&current).unwrap();
        stack_activation
            .with(|runtime, stack| {
                push_probe(stack, runtime, &current, probe, &value).unwrap();
                let error = stack
                    .start_native_entry(runtime, |runtime, _, _, _, destination| {
                        destination.publish(runtime, &ValueView::unit())
                    })
                    .unwrap_err();
                assert_eq!(error.kind(), RuntimeErrorKind::ModuleValidation);
                assert!(error.to_string().contains("native callback result"));

                Ok(())
            })
            .unwrap();
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    let mut stack_activation = runtime.enter_execution_stack(&current).unwrap();
    stack_activation
        .with(|runtime, stack| {
            push_probe(stack, runtime, &current, probe, &value).unwrap();
            drop(value);
            stack.start_native_entry(runtime, invoke_stored).unwrap();
            let ReturnAction::Complete(result) = stack.finish_native_return(runtime).unwrap()
            else {
                panic!("probe return")
            };
            stack
                .with_result(runtime, result, |_, result| {
                    assert_eq!(result.as_i64(), Some(11));
                    Ok(())
                })
                .unwrap();

            Ok(())
        })
        .unwrap();
    drop(stack_activation);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
    runtime.collect_garbage().unwrap();
    assert!(
        runtime.gc_stats().objects < before_probe.objects,
        "the released payload and its selected-source provider are collectible"
    );
    assert_eq!(
        runtime.gc_stats().root_registrations + 1,
        before_probe.root_registrations
    );
    assert!(!runtime.is_quarantined());
}

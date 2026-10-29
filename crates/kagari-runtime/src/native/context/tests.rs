use crate::{
    Runtime,
    error::{RuntimeError, RuntimeErrorKind},
    frame::driver::ReturnAction,
    module::{ExecutableModule, Program},
    native::{
        binding::{Codec, LinkedNativeFunction, NativeBinding, NativeResult},
        builder::ModuleBuilder,
        catalog::DeclarationCatalog,
        context::{
            CallContext, CallableOwner, LinkedCallable, LinkedOperation, ScriptCall, ScriptInvoker,
            arguments::{ArgumentSlots, ArgumentView},
        },
        declarations::FunctionDecl,
        storage::NativeStorage,
        stored_selection::StoredSelection,
        types::Type,
    },
    test_support::native::program::free_function_program,
    value::Value,
};
use kagari_bytecode::{instruction::NativeImportId, module::CallableTarget};
use kagari_types::{scalar::BuiltinType, ty::Ty};
use std::{
    cell::Cell,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

fn payload_factory() -> (Runtime, Program, Arc<LinkedNativeFunction>) {
    let mut builder = ModuleBuilder::new("test::factory", &DeclarationCatalog::default()).unwrap();
    let mut data = builder.define_type("Payload").unwrap();
    data.native_storage(NativeStorage::data::<u32>()).unwrap();
    let payload = data.finish().unwrap();
    let make = builder
        .define_function(FunctionDecl::new("make").returns(payload.apply([]).unwrap()))
        .unwrap();
    builder
        .bind(
            make,
            NativeBinding::new(vec![], payload.codec(), |_| unreachable!()),
        )
        .unwrap();
    let native = builder.finish().unwrap();
    let mut runtime = Runtime::default();
    native.install(&mut runtime).unwrap();
    let program = runtime
        .load_program("factory", free_function_program(&native.to_declaration()))
        .unwrap();
    let owner = runtime.select_program(&program).unwrap();
    let function = runtime
        .modules
        .native_binding(&owner, NativeImportId::new(0))
        .unwrap();
    (runtime, program, function)
}

fn factory_call<'a>(
    runtime: &'a Runtime,
    owner: &'a ExecutableModule,
    function: &'a LinkedNativeFunction,
) -> CallContext<'a> {
    CallContext {
        runtime,
        owner,
        function,
        arguments: ArgumentView {
            heap: runtime.gc(),
            frame: None,
            slots: ArgumentSlots::Scalars(&[]),
            narrowed: None,
        },
        invoke_script: unsafe {
            // SAFETY: The callback rejects execution or returns only a scalar; it retains no raw edges.
            ScriptInvoker::new(|_, _, _, _| unreachable!())
        },
    }
}

#[test]
fn payload_builder_preflight_rejects_mapping_and_occurrence_before_effects() {
    let (runtime, program, function) = payload_factory();
    let owner = runtime.select_program(&program).unwrap();
    let call = factory_call(&runtime, &owner, &function);
    let effects = Cell::new(0);
    let error = call
        .allocate_result_payload(&[], |_, _| {
            effects.set(effects.get() + 1);
            Ok(7_u64)
        })
        .unwrap_err();
    assert!(error.message().contains("payload type mismatch"));
    let mut wrong = function.as_ref().clone();
    wrong.import = NativeImportId::new(1);
    let error = factory_call(&runtime, &owner, &wrong)
        .allocate_result_payload(&[], |_, _| {
            effects.set(effects.get() + 1);
            Ok(7_u32)
        })
        .unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::ModuleValidation);
    let mut wrong = function.as_ref().clone();
    let mut signature = wrong.prepared_signature.take().unwrap();
    signature.result = runtime
        .type_arguments(&owner, None, &[Ty::Builtin(BuiltinType::I32)])
        .unwrap()
        .pop()
        .unwrap();
    wrong.prepared_signature.set(signature).unwrap();
    assert!(
        factory_call(&runtime, &owner, &wrong)
            .allocate_result_payload(&[], |_, _| {
                effects.set(effects.get() + 1);
                Ok(7_u32)
            })
            .is_err()
    );
    assert_eq!(effects.get(), 0);
    assert_eq!(runtime.gc().active_roots(), 0);
}

#[test]
fn payload_builder_preflight_rejects_unavailable_and_faulted_authority_before_effects() {
    enum Rejected {
        Foreign,
        Abandoned,
        Condemned,
        Faulted,
        Poisoned,
    }

    for rejected in [
        Rejected::Foreign,
        Rejected::Abandoned,
        Rejected::Condemned,
        Rejected::Faulted,
        Rejected::Poisoned,
    ] {
        let (runtime, program, function) = payload_factory();
        let original = runtime.select_program(&program).unwrap();
        let owner = match rejected {
            Rejected::Foreign => {
                let (foreign, program, _) = payload_factory();
                foreign.select_program(&program).unwrap()
            }
            Rejected::Abandoned => {
                let staged = runtime
                    .stage_reload_verified_program(
                        &program,
                        None,
                        original.verified_program().clone(),
                    )
                    .unwrap();
                let owner = staged.module().clone();
                drop(staged);
                owner
            }
            Rejected::Condemned => {
                let mut marker = runtime.gc.graph.lock();
                marker.begin_cycle().unwrap();
                marker.finish_roots().unwrap();
                marker.close_mark().unwrap();
                original
            }
            Rejected::Faulted => {
                runtime.gc.graph.lock().fault();
                // Availability for diagnostics is not permission to build a payload.
                runtime.validate_executable_module(&original).unwrap();
                original
            }
            Rejected::Poisoned => {
                assert!(
                    catch_unwind(AssertUnwindSafe(|| {
                        let _marker = runtime.gc.graph.lock();
                        panic!("injected factory graph poison");
                    }))
                    .is_err()
                );
                original
            }
        };
        let effects = Cell::new(0);
        assert!(
            factory_call(&runtime, &owner, &function)
                .allocate_result_payload(&[], |_, _| {
                    effects.set(effects.get() + 1);
                    Ok(7_u32)
                })
                .is_err()
        );
        assert_eq!(effects.get(), 0);
    }
}

#[test]
fn payload_builder_precedes_selected_slots_and_preserves_completed_effects() {
    let (runtime, program, function) = payload_factory();
    let owner = runtime.select_program(&program).unwrap();
    let call = factory_call(&runtime, &owner, &function);
    let effects = Cell::new(0);
    let error = call
        .allocate_result_payload::<u32>(&[0], |_, _| {
            effects.set(effects.get() + 1);
            Err(RuntimeError::module_validation("builder failed"))
        })
        .unwrap_err();
    assert_eq!(error.message(), "module validation failed: builder failed");
    let error = call
        .allocate_result_payload(&[0], |_, _| {
            effects.set(effects.get() + 1);
            Ok(7_u32)
        })
        .unwrap_err();
    assert!(error.message().contains("selected callable slot"));
    assert_eq!(effects.get(), 2);
    assert_eq!(runtime.gc().active_roots(), 0);
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 0);
}

#[test]
fn payload_builder_effects_do_not_let_allocation_reuse_expired_admission() {
    let (runtime, program, function) = payload_factory();
    let owner = runtime.select_program(&program).unwrap();
    let effects = Cell::new(0);
    assert!(
        factory_call(&runtime, &owner, &function)
            .allocate_result_payload(&[], |_, _| {
                effects.set(effects.get() + 1);
                runtime.gc.graph.lock().fault();
                Ok(7_u32)
            })
            .is_err()
    );
    assert_eq!(effects.get(), 1);
    assert_eq!(runtime.gc().active_roots(), 0);
}

#[test]
fn payload_builder_borrows_call_authority_and_result_survives_without_factory_program() {
    let (runtime, program, function) = payload_factory();
    let owner = runtime.select_program(&program).unwrap();
    let session = runtime
        .begin_execution(&owner, runtime.execution_options())
        .unwrap();
    let before = runtime.modules().retention_counts(owner.key());
    let roots = runtime.gc().active_roots();
    let call = factory_call(&runtime, &owner, &function);
    let value = call
        .allocate_result_payload(&[], |call, argument| {
            assert_eq!(argument.ty(), call.result_type());
            let during = runtime.modules().retention_counts(owner.key());
            assert_eq!(during.runtime_values, before.runtime_values);
            assert_eq!(during.active_calls, before.active_calls);
            assert_eq!(runtime.gc().active_roots(), roots);
            // An actual owning factory has a distinct lease. Immediate construction
            // must not manufacture that lease, even temporarily around the builder.
            {
                let native = call.native_context()?;
                let factory = native.result_native_type::<u32>()?;
                assert_eq!(
                    runtime
                        .modules()
                        .retention_counts(owner.key())
                        .runtime_values,
                    before.runtime_values + 1
                );
                drop(factory);
            }
            runtime.modules().check_sweep_access()?;
            call.collect_garbage()?;
            assert_eq!(
                runtime
                    .modules()
                    .retention_counts(owner.key())
                    .runtime_values,
                before.runtime_values
            );
            Ok(42_u32)
        })
        .unwrap();
    let root = runtime.root_value(value).unwrap();
    assert_eq!(
        runtime
            .modules()
            .retention_counts(owner.key())
            .runtime_values,
        before.runtime_values
    );
    drop(session);
    let staged = runtime
        .stage_reload_verified_program(&program, None, owner.verified_program().clone())
        .unwrap();
    runtime.publish_staged_reload(staged).unwrap();
    // Edge-free data retains immutable type facts, not the factory's executable
    // program. Only stored executable dependencies would keep that program alive.
    assert!(
        runtime
            .collect_garbage()
            .unwrap()
            .reclaimed_modules
            .contains(&owner.key())
    );
    let Value::GcHandle(id) = root.value(runtime.gc()).unwrap() else {
        panic!("native result")
    };
    assert_eq!(
        runtime
            .gc()
            .with_native::<u32, _>(id, |payload| Ok(*payload))
            .unwrap(),
        42
    );
    drop(root);
    let collected = runtime.collect_garbage().unwrap();
    assert_eq!(collected.reclaimed_objects, 1);
    assert_eq!(collected.live_objects, 0);
}

fn invoke_stored(
    runtime: &Runtime,
    owner: &ExecutableModule,
    target: ScriptCall<'_>,
    arguments: &[Value],
) -> NativeResult<Value> {
    // Stored ordinary callbacks select the current publication while keeping the
    // enclosing root session and its candidate/installation admission intact.
    assert_eq!(runtime.execution_root().unwrap().key(), owner.key());
    let ScriptCall::Pinned(function) = target else {
        panic!("stored selection requires retained entry authority")
    };
    let stack = runtime.enter_pinned_execution_stack(function)?;
    stack.push_pinned_call(runtime, function, arguments)?;
    stack.start_native_entry(runtime, unsafe {
        // SAFETY: The helper admits the selected target, roots its frame and transfers the terminal result immediately.
        ScriptInvoker::new(invoke_stored)
    })?;
    let ReturnAction::Complete(value) = stack.finish_native_return(runtime)? else {
        panic!("stored native return")
    };
    Ok(value)
}

fn call_scalar_equal(
    runtime: &Runtime,
    owner: &ExecutableModule,
    target: &LinkedCallable,
) -> NativeResult<bool> {
    let CallableTarget::Native(import) = target.target else {
        unreachable!()
    };
    let function = runtime.modules.native_binding(owner, import).unwrap();
    let mut context = CallContext {
        runtime,
        owner,
        function: &function,
        arguments: ArgumentView {
            heap: runtime.gc(),
            frame: None,
            slots: ArgumentSlots::Scalars(&[]),
            narrowed: None,
        },
        invoke_script: unsafe {
            // SAFETY: The callback rejects execution or returns only a scalar; it retains no raw edges.
            ScriptInvoker::new(|_, _, _, _| panic!("scalar native callback must use direct entry"))
        },
    };
    context.call(target, (7_i32, 7_i32))
}

#[test]
fn stored_selections_select_current_with_frame_roots_and_checked_calls() {
    let mut builder = ModuleBuilder::new("test::stored", &DeclarationCatalog::default()).unwrap();
    let mut data = builder.define_type("SelectionOwner").unwrap();
    data.native_storage(NativeStorage::data::<u32>()).unwrap();
    let collection = data.finish().unwrap().apply([]).unwrap();
    let probe = builder
        .define_function(
            FunctionDecl::new("probe")
                .parameter("collection", collection)
                .returns(Type::i64()),
        )
        .unwrap();
    builder
        .bind(
            probe,
            NativeBinding::new(vec![Codec::Value], Codec::Value, |cx| {
                let receiver = cx.argument(0)?;
                cx.collect_garbage()?;
                assert!(cx.heap().validate_value(&receiver));
                assert_eq!(
                    cx.call_stored_values(0, 1, &[Value::I32(7), Value::I32(7)])?
                        .value(cx.heap())
                        .unwrap(),
                    Value::Bool(true)
                );
                for (slot, args) in [
                    (2, vec![Value::I32(7)]),
                    (0, vec![]),
                    (0, vec![Value::Bool(true)]),
                ] {
                    assert_eq!(
                        cx.call_stored_values(0, slot, &args).unwrap_err().kind(),
                        RuntimeErrorKind::ModuleValidation
                    );
                }
                let result = cx.call_stored_values(0, 0, &[Value::I32(7)])?;
                assert!(cx.heap().validate_value(&receiver));
                Ok(result.value(cx.heap()).unwrap())
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
                Ok(Value::I64(11))
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
                Ok(Value::Bool(cx.argument(0)? == cx.argument(1)?))
            }),
        )
        .unwrap();
    let native = builder.finish().unwrap();
    let code = free_function_program(&native.to_declaration());
    let mut runtime = Runtime::default();
    native.install(&mut runtime).unwrap();
    let old_program = runtime.load_program("stored", code.clone()).unwrap();
    let old = runtime.select_program(&old_program).unwrap();
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
            scoped_signature: None,
            owner: CallableOwner::Program(old.slot()),
            target: CallableTarget::Native(import(name)),
            params: params.into(),
            result,
            primitive: None,
        })
    });
    let ty = old.bytecode.native_imports[probe.index()].signature.params[0].clone();
    let mut object = NativeStorage::data::<u32>()
        .prepare_payload(runtime.gc(), &ty, 0u32, &old)
        .unwrap();
    object.selected = StoredSelection::slots(&old, &selected, &[0, 1]).unwrap();
    let id = runtime.gc().alloc_native(object).unwrap();
    let value = Value::GcHandle(id);
    let root = runtime.root_value(value).unwrap();
    let staged = runtime
        .stage_reload_program(&old_program, None, code.clone())
        .unwrap();
    runtime.publish_staged_reload(staged).unwrap();

    let current = runtime.select_program(&old_program).unwrap();
    *expected_publication.lock().unwrap() = Some(current.publication_token());
    {
        let _stack = runtime.enter_execution_stack(&current).unwrap();
        assert!(call_scalar_equal(&runtime, &old, selected[1].ready().unwrap()).unwrap());
    }
    assert!(
        !runtime
            .collect_garbage()
            .unwrap()
            .reclaimed_modules
            .contains(&old.key())
    );

    // A candidate must not use a stored edge to enter a published generation.
    let candidate = runtime
        .stage_reload_program(&old_program, None, code.clone())
        .unwrap();
    {
        let _session = runtime.begin_candidate_initialization(&candidate).unwrap();
        assert_eq!(
            call_scalar_equal(&runtime, &old, selected[1].ready().unwrap())
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::ExecutionPhaseViolation
        );
        let stack = runtime.enter_execution_stack(candidate.module()).unwrap();
        assert_eq!(
            stack
                .push_callable(
                    &runtime,
                    candidate.module().clone(),
                    CallableTarget::Native(probe),
                    &[value],
                    None,
                )
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::ExecutionPhaseViolation
        );
    }
    drop(candidate);
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    let mut foreign = Runtime::default();
    native.install(&mut foreign).unwrap();
    let foreign_owner_program = foreign.load_program("stored", code).unwrap();
    let foreign_owner = foreign.select_program(&foreign_owner_program).unwrap();
    {
        let stack = foreign.enter_execution_stack(&foreign_owner).unwrap();
        assert!(
            stack
                .push_callable(
                    &foreign,
                    foreign_owner,
                    CallableTarget::Native(probe),
                    &[value],
                    None
                )
                .is_err()
        );
    }
    // Even a backend that returns a value of the wrong type cannot publish it.
    {
        let stack = runtime.enter_execution_stack(&current).unwrap();
        stack
            .push_callable(
                &runtime,
                current.clone(),
                CallableTarget::Native(probe),
                &[value],
                None,
            )
            .unwrap();
        let error = stack
            .start_native_entry(&runtime, unsafe {
                // SAFETY: The callback rejects execution or returns only a scalar; it retains no raw edges.
                ScriptInvoker::new(|_, _, _, _| Ok(Value::Unit))
            })
            .unwrap_err();
        assert_eq!(error.kind(), RuntimeErrorKind::ModuleValidation);
        assert!(error.message().contains("selected call result"));
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    let stack = runtime.enter_execution_stack(&current).unwrap();
    stack
        .push_callable(
            &runtime,
            current.clone(),
            CallableTarget::Native(probe),
            &[value],
            None,
        )
        .unwrap();
    drop(root);
    stack
        .start_native_entry(&runtime, unsafe {
            // SAFETY: The helper admits the selected target, roots its frame and transfers the terminal result immediately.
            ScriptInvoker::new(invoke_stored)
        })
        .unwrap();
    let ReturnAction::Complete(result) = stack.finish_native_return(&runtime).unwrap() else {
        panic!("probe return")
    };
    assert_eq!(result, Value::I64(11));
    drop(stack);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
    assert_eq!(runtime.gc().active_roots(), 0);
    let collected = runtime.collect_garbage().unwrap();
    assert!(collected.reclaimed_modules.contains(&old.key()));
    assert_eq!(collected.reclaimed_objects, 1);
    assert_eq!(collected.live_objects, 0);
    // A direct address cannot be inspected after its retention ends. Exercise
    // argument rejection with a valid scalar of the wrong type instead.
    let stack = runtime.enter_execution_stack(&current).unwrap();
    assert!(
        stack
            .push_callable(
                &runtime,
                current,
                CallableTarget::Native(probe),
                &[Value::I64(0)],
                None
            )
            .is_err()
    );
}

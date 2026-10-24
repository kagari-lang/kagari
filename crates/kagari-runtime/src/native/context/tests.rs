use crate::{
    Runtime,
    error::RuntimeErrorKind,
    frame::driver::ReturnAction,
    module::ExecutableModule,
    native::{
        binding::{Codec, NativeBinding, NativeResult},
        builder::ModuleBuilder,
        catalog::DeclarationCatalog,
        context::{
            ArgumentSlots, ArgumentView, CallContext, CallableOwner, LinkedCallable,
            LinkedOperation, ScriptCall,
        },
        declarations::FunctionDecl,
        storage::{NativeStorage, StorageContext},
        types::Type,
    },
    test_support::native::program::free_function_program,
    value::Value,
};
use kagari_bytecode::{instruction::NativeImportId, module::CallableTarget};
use kagari_types::{collection::CollectionAccess, scalar::BuiltinType, ty::Ty};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

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
    stack.start_native_entry(runtime, invoke_stored)?;
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
        invoke_script: |_, _, _, _| panic!("scalar native callback must use direct entry"),
    };
    context.call(target, (7_i32, 7_i32))
}

#[test]
fn stored_hash_selections_select_current_with_frame_roots_and_checked_calls() {
    for is_map in [true, false] {
        let mut builder =
            ModuleBuilder::new("test::stored", &DeclarationCatalog::default()).unwrap();
        let collection = if is_map {
            Type::map(Type::i32(), Type::i32(), CollectionAccess::Mutable).unwrap()
        } else {
            Type::set(Type::i32(), CollectionAccess::Mutable).unwrap()
        };
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
                    assert!(!cx.heap().uses_builtin_keys(&receiver)?);
                    cx.collect_garbage()?;
                    assert!(cx.heap().validate_value(&receiver));
                    assert_eq!(
                        cx.call_stored_values(0, 1, &[Value::I32(7), Value::I32(7)])?,
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
        let storage = if is_map {
            NativeStorage::map(0, 1)
        } else {
            NativeStorage::set(0)
        };
        let object = storage
            .create(&StorageContext {
                runtime: &runtime,
                owner: &old,
                ty: &ty,
                scope: None,
                selected: &selected,
            })
            .unwrap();
        let id = runtime.gc().alloc_native(object).unwrap();
        let value = if is_map {
            Value::Map(id)
        } else {
            Value::Set(id)
        };
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
                .start_native_entry(&runtime, |_, _, _, _| Ok(Value::Unit))
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
        stack.start_native_entry(&runtime, invoke_stored).unwrap();
        let ReturnAction::Complete(result) = stack.finish_native_return(&runtime).unwrap() else {
            panic!("probe return")
        };
        assert_eq!(result, Value::I64(11));
        drop(stack);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(runtime.resources().counters().current_call_depth, 0);
        assert_eq!(runtime.gc().active_roots(), 0);
        assert!(
            runtime
                .collect_garbage()
                .unwrap()
                .reclaimed_modules
                .contains(&old.key())
        );
        assert!(!runtime.gc().validate_value(&value));
        let stack = runtime.enter_execution_stack(&current).unwrap();
        assert!(
            stack
                .push_callable(
                    &runtime,
                    current,
                    CallableTarget::Native(probe),
                    &[value],
                    None
                )
                .is_err()
        );
    }
}

//! Owned Host passing checks nested loans through actual frame/source admission.
use crate::{
    Runtime,
    error::{RuntimeError, RuntimeErrorKind},
    execution::roots::ExecutionRoots,
    frame::driver::ReturnAction,
    host::{HostBorrowKind, HostObjectId},
    module::ExecutableModule,
    native::{
        builder::ModuleBuilder,
        conversion::{arguments::ArgumentValues, context::ConversionContext},
        declarations::FunctionDecl,
        host::callback,
        types::Type,
    },
    test_support::{bytecode::load_bytecode, native::program::free_function_program},
    value::{
        EphemeralValue, aggregates::tuples::TupleObject, carriers::ScopedValue,
        host_values::EphemeralObject, view::ValueView,
    },
};
use kagari_bytecode::{instruction::NativeImportId, module::CallableTarget};
use kagari_types::declaration::host::{
    HostCallPolicy, HostFunctionEffects, HostPassingStyle, HostReflectionPolicy, HostTypeOwnership,
    HostTypePolicy, PathAccess,
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

fn invoke_borrowed(
    runtime: &mut Runtime,
    owner: &ExecutableModule,
    slot: usize,
    arguments: ArgumentValues<'_>,
) -> Result<(), RuntimeError> {
    let mut activation = runtime.enter_execution_stack(owner)?;
    activation.with(|runtime, stack| {
        stack.push_callable(
            runtime,
            owner.clone(),
            CallableTarget::Native(NativeImportId::new(slot)),
            arguments,
            None,
        )?;
        stack.start_native_entry(runtime, |_, _, _, _, _| unreachable!("no script callback"))?;
        assert!(matches!(
            stack.finish_native_return(runtime)?,
            ReturnAction::Complete(_)
        ));
        Ok(())
    })
}

#[test]
fn owned_composites_cannot_hide_frame_scoped_host_borrows() {
    let mut runtime = Runtime::default();
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
    let value = object.apply([]).unwrap();
    let nested = Type::tuple([Type::tuple([value.clone()]).unwrap()]).unwrap();
    let consume = builder
        .define_function(
            FunctionDecl::new("consume")
                .parameter("value", nested)
                .host_policy(HostCallPolicy {
                    passing: vec![HostPassingStyle::Owned],
                    effects: HostFunctionEffects {
                        may_allocate: true,
                        ..Default::default()
                    },
                }),
        )
        .unwrap();
    builder
        .bind(
            consume,
            callback(|_, _| {
                panic!("owned passing must reject the nested loan before callback effects")
            }),
        )
        .unwrap();
    let mut other = builder.define_type("Other").unwrap();
    other
        .external(HostTypePolicy {
            ownership: HostTypeOwnership::HostRoot,
            reflection: HostReflectionPolicy::Hidden,
            path_access: PathAccess::None,
        })
        .unwrap();
    other.finish().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let shared = builder
        .define_function(
            FunctionDecl::new("shared")
                .parameter("value", value)
                .host_policy(HostCallPolicy {
                    passing: vec![HostPassingStyle::SharedBorrow],
                    effects: Default::default(),
                }),
        )
        .unwrap();
    builder
        .bind(
            shared,
            callback(move |_, _| {
                observed.fetch_add(1, Ordering::SeqCst);
                Ok(ValueView::unit())
            }),
        )
        .unwrap();
    let provider = builder.finish().unwrap();
    let program = free_function_program(&provider.to_declaration());
    provider.install(&mut runtime).unwrap();
    let installed = load_bytecode(&mut runtime, "owned-composite", program).unwrap();
    let owner = runtime.select_program(&installed).unwrap();
    let type_id = runtime
        .types()
        .get_by_name("fixtures::escape::Object")
        .unwrap()
        .id;
    let other_type = runtime
        .types()
        .get_by_name("fixtures::escape::Other")
        .unwrap()
        .id;
    let expected = runtime
        .resolve_type_arguments(&owner, &owner.bytecode.native_imports[0].signature.params)
        .unwrap()
        .remove(0);
    runtime.collect_garbage().unwrap();
    let baseline = runtime.gc_stats();
    for unique in [false, true] {
        let mut scope = runtime.host_scope(&[]).unwrap();
        let access = if unique {
            HostBorrowKind::Unique
        } else {
            HostBorrowKind::Shared
        };
        let token = if unique {
            scope
                .borrows()
                .borrow_unique(HostObjectId(1), type_id)
                .unwrap()
        } else {
            scope
                .borrows()
                .borrow_shared(HostObjectId(1), type_id)
                .unwrap()
        };
        let execution = scope.execution_id();
        let nested = {
            let Runtime {
                memory, resources, ..
            } = scope.runtime_mut();
            memory.scope(resources, |cx, roots| {
                ExecutionRoots::with(roots, cx, |slots| {
                    let borrowed = EphemeralObject::alloc(
                        slots.scope(),
                        if unique {
                            EphemeralValue::HostMut(token)
                        } else {
                            EphemeralValue::HostRef(token)
                        },
                    )?;
                    let inner = TupleObject::alloc(slots.scope(), &[borrowed])?;
                    let outer = TupleObject::alloc(slots.scope(), &[ScopedValue::managed(inner)])?;
                    slots.protect_local(execution, ScopedValue::managed(outer))
                })
            })
        }
        .unwrap();
        scope.runtime_mut().collect_garbage().unwrap();
        assert_eq!(scope.runtime().gc_stats().objects, baseline.objects + 3);
        {
            let mut cx = ConversionContext::new(scope.runtime_mut(), &owner).unwrap();
            // The declared nested nominal type is valid. The failing admission
            // below must therefore distinguish Owned passing from type rejection.
            cx.check_value(&expected, &nested).unwrap();
            let arguments = cx.argument_values(&[nested]).unwrap();
            let error = invoke_borrowed(cx.runtime_mut(), &owner, 0, arguments).unwrap_err();
            assert_eq!(error.kind(), RuntimeErrorKind::HostBorrowEscape);
            // The matching Host-containing tuple is a valid call parameter but
            // is not a script-heap element type, independently of its payload.
            let error = cx.allocate_array(&expected, &[nested]).unwrap_err();
            assert_eq!(error.kind(), RuntimeErrorKind::ModuleValidation);
            assert!(error.to_string().contains("closed script-heap type"));
        }
        assert_eq!(
            scope.retain(&nested).unwrap_err().kind(),
            RuntimeErrorKind::HostBorrowEscape
        );
        // Real shared/unique tokens admit only their declared nominal type,
        // independently of the common ephemeral carrier and passing category.
        let wrong = if unique {
            scope
                .borrows()
                .borrow_unique(HostObjectId(2), other_type)
                .unwrap()
        } else {
            scope
                .borrows()
                .borrow_shared(HostObjectId(2), other_type)
                .unwrap()
        };
        for (token, valid) in [(token, true), (wrong, false)] {
            let value = {
                let Runtime {
                    memory, resources, ..
                } = scope.runtime_mut();
                memory.scope(resources, |cx, roots| {
                    ExecutionRoots::with(roots, cx, |slots| {
                        let value = EphemeralObject::alloc(
                            slots.scope(),
                            if unique {
                                EphemeralValue::HostMut(token)
                            } else {
                                EphemeralValue::HostRef(token)
                            },
                        )?;
                        slots.protect_local(execution, value)
                    })
                })
            }
            .unwrap();
            let before = calls.load(Ordering::SeqCst);
            let mut cx = ConversionContext::new(scope.runtime_mut(), &owner).unwrap();
            let arguments = cx.argument_values(&[value]).unwrap();
            let result = invoke_borrowed(cx.runtime_mut(), &owner, 1, arguments);
            if valid {
                result.unwrap();
                assert_eq!(calls.load(Ordering::SeqCst), before + 1);
            } else {
                assert_eq!(
                    result.unwrap_err().kind(),
                    RuntimeErrorKind::ModuleValidation
                );
                assert_eq!(calls.load(Ordering::SeqCst), before);
            }
        }
        scope.borrows().validate(token, access).unwrap();
        drop(scope);
        assert_eq!(
            runtime
                .validate_host_borrow(token, access)
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::ExpiredHostBorrow
        );
        runtime.collect_garbage().unwrap();
        assert_eq!(runtime.gc_stats().objects, baseline.objects);
        assert_eq!(
            runtime.gc_stats().root_registrations,
            baseline.root_registrations
        );
        assert!(!runtime.is_quarantined());
    }
}

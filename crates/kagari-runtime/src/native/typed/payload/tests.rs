#[cfg(feature = "execution-diagnostics")]
use crate::diagnostics::allocations;
use crate::{
    Runtime,
    error::{RuntimeError, RuntimeErrorKind},
    gc::roots::RootedValue,
    module::Program,
    native::{
        binding::NativeBinding, builder::ModuleBuilder, catalog::DeclarationCatalog,
        declarations::FunctionDecl, storage_type::StorageType, typed::NativeContext,
        value_handle::ScriptValue, views::NativeOperationKind,
    },
    test_support::native::{invoke::invoke, program::free_function_program},
    value::Value,
};
use std::panic::{AssertUnwindSafe, catch_unwind};

fn payload() -> (Runtime, Program, RootedValue, [StorageType; 2]) {
    let mut builder =
        ModuleBuilder::new("scoped::payload", &DeclarationCatalog::default()).unwrap();
    let first = builder
        .define_opaque::<u32>("First")
        .unwrap()
        .finish()
        .unwrap();
    let second = builder
        .define_opaque::<u64>("Second")
        .unwrap()
        .finish()
        .unwrap();
    let make = builder
        .define_function(FunctionDecl::new("make").returns(first.apply([]).unwrap()))
        .unwrap();
    builder
        .bind(
            make,
            NativeBinding::new([], first.codec(), |call| {
                call.allocate_result_payload(&[], |_, _| Ok(7_u32))
            }),
        )
        .unwrap();
    let other = builder
        .define_function(FunctionDecl::new("other").returns(second.apply([]).unwrap()))
        .unwrap();
    builder
        .bind(
            other,
            NativeBinding::new([], second.codec(), |call| {
                call.allocate_result_payload(&[], |_, _| Ok(9_u64))
            }),
        )
        .unwrap();
    let native = builder.finish().unwrap();
    let mut runtime = Runtime::default();
    native.install(&mut runtime).unwrap();
    let program = runtime
        .load_program("scoped", free_function_program(&native.to_declaration()))
        .unwrap();
    let owner = runtime.select_program(&program).unwrap();
    let value = invoke(&runtime, &owner, 0, &[]).unwrap();
    let root = runtime.root_value(value).unwrap();
    let contracts = [0, 1].map(|index| {
        let argument = runtime
            .type_arguments(
                &owner,
                None,
                &[owner.bytecode.native_imports[index]
                    .signature
                    .result
                    .clone()],
            )
            .unwrap()
            .pop()
            .unwrap();
        StorageType::prepare_scoped(argument, &owner).unwrap()
    });
    (runtime, program, root, contracts)
}

#[test]
fn scoped_payload_read_and_contract_clone_add_no_roots_or_execution_leases() {
    let (runtime, program, root, [contract, _]) = payload();
    let owner = runtime.select_program(&program).unwrap();
    let cx = NativeContext::new(&runtime, &owner).unwrap();
    let roots = runtime.gc().active_roots();
    let leases = runtime.modules().retention_counts(owner.key());
    #[cfg(feature = "execution-diagnostics")]
    let clone = {
        let (clone, counts) = allocations::measure(|| contract.clone());
        assert_eq!(counts.requests, 0);
        assert_eq!(counts.net_bytes, 0);
        clone
    };
    #[cfg(not(feature = "execution-diagnostics"))]
    let clone = contract.clone();
    drop(contract);
    runtime.collect_garbage().unwrap();
    // SAFETY: The existing root protects the value and only copied fixed data escapes.
    let result = unsafe {
        cx.read_payload::<u32, _>(root.value(runtime.gc()).unwrap(), &clone, |value, _| {
            Ok(*value)
        })
    };
    assert_eq!(result.unwrap(), 7);
    assert_eq!(runtime.gc().active_roots(), roots);
    assert_eq!(runtime.modules().retention_counts(owner.key()), leases);
    drop(cx);
    let candidate = runtime
        .stage_reload_verified_program(&program, None, owner.verified_program().clone())
        .unwrap();
    runtime.publish_staged_reload(candidate).unwrap();
    let current = runtime.select_program(&program).unwrap();
    let cx = NativeContext::new(&runtime, &current).unwrap();
    runtime.collect_garbage().unwrap();
    assert!(runtime.modules().loaded(owner.key()).is_none());
    // SAFETY: Data stays rooted; the old lexical contract needs no executable lease.
    assert_eq!(
        unsafe {
            cx.read_payload::<u32, _>(root.value(runtime.gc()).unwrap(), &clone, |value, _| {
                Ok(*value)
            })
        }
        .unwrap(),
        7
    );
}

#[test]
fn scoped_payload_read_validates_contract_identity_and_mapping_before_callback() {
    let (runtime, program, root, [contract, wrong]) = payload();
    let owner = runtime.select_program(&program).unwrap();
    let cx = NativeContext::new(&runtime, &owner).unwrap();
    let value = root.value(runtime.gc()).unwrap();
    // SAFETY: Invalid inputs must fail before lending a payload; the valid value is rooted.
    unsafe {
        let error = cx
            .read_payload::<u32, ()>(value, &wrong, |_, _| panic!("wrong nominal admitted"))
            .unwrap_err();
        assert!(error.message().contains("differs from its closed contract"));
        let error = cx
            .read_payload::<u64, ()>(value, &contract, |_, _| panic!("wrong Rust type admitted"))
            .unwrap_err();
        assert!(error.message().contains("Rust host target"));
        let wrong_kind = runtime.gc().alloc_string("not native".into()).unwrap();
        assert!(
            cx.read_payload::<u32, ()>(wrong_kind, &contract, |_, _| panic!())
                .is_err()
        );
        let (foreign, foreign_program, foreign_root, [foreign_contract, _]) = payload();
        let foreign_owner = foreign.select_program(&foreign_program).unwrap();
        let foreign_cx = NativeContext::new(&foreign, &foreign_owner).unwrap();
        let error = foreign_cx
            .read_payload::<u32, ()>(value, &contract, |_, _| panic!("foreign scope admitted"))
            .unwrap_err();
        assert!(
            error
                .message()
                .contains("definition belongs to a different identity table"),
            "{error}"
        );
        assert!(
            cx.read_payload::<u32, ()>(
                foreign_root.value(foreign.gc()).unwrap(),
                &contract,
                |_, _| panic!()
            )
            .is_err()
        );
        assert!(
            foreign_cx
                .read_payload::<u32, ()>(value, &foreign_contract, |_, _| panic!())
                .is_err()
        );
    }
    drop(root);
    runtime.collect_garbage().unwrap();
    // SAFETY: Stale handles must reject before touching payload storage.
    assert!(unsafe { cx.read_payload::<u32, ()>(value, &contract, |_, _| panic!()) }.is_err());
    let replacement = invoke(&runtime, &owner, 0, &[]).unwrap();
    let _replacement_root = runtime.root_value(replacement).unwrap();
    assert_ne!(replacement, value);
    // SAFETY: Reuse must not make the old full identity accessible.
    assert!(unsafe { cx.read_payload::<u32, ()>(value, &contract, |_, _| panic!()) }.is_err());
}

#[test]
fn scoped_payload_read_preserves_phase_admission_and_source_error_precedence() {
    enum Rejected {
        Faulted,
        Poisoned,
        Sweep,
        Candidate,
        Closed,
    }
    for phase in [
        Rejected::Faulted,
        Rejected::Poisoned,
        Rejected::Sweep,
        Rejected::Candidate,
        Rejected::Closed,
    ] {
        let (runtime, program, root, [contract, wrong]) = payload();
        let owner = runtime.select_program(&program).unwrap();
        let cx = NativeContext::new(&runtime, &owner).unwrap();
        let value = root.value(runtime.gc()).unwrap();
        let candidate = matches!(phase, Rejected::Candidate).then(|| {
            runtime
                .stage_reload_verified_program(&program, None, owner.verified_program().clone())
                .unwrap()
        });
        let _initialization = candidate
            .as_ref()
            .map(|candidate| runtime.begin_candidate_initialization(candidate).unwrap());
        match phase {
            Rejected::Faulted => {
                runtime.gc.graph.lock().fault();
                // Source contract rejection precedes executable/mapping publication.
                let error = unsafe { cx.read_payload::<u64, ()>(value, &wrong, |_, _| panic!()) }
                    .unwrap_err();
                assert!(error.message().contains("differs from its closed contract"));
            }
            Rejected::Poisoned => {
                assert!(
                    catch_unwind(AssertUnwindSafe(|| {
                        let _marker = runtime.gc.graph.lock();
                        panic!("injected scoped access poison");
                    }))
                    .is_err()
                );
            }
            Rejected::Sweep => {
                let mut marker = runtime.gc.graph.lock();
                marker.begin_cycle().unwrap();
                marker.finish_roots().unwrap();
                marker.close_mark().unwrap();
            }
            Rejected::Candidate => {}
            Rejected::Closed => runtime.resources().close_execution(),
        }
        // SAFETY: Every unavailable phase must reject before the closure or wrong-P mapping.
        let error =
            unsafe { cx.read_payload::<u64, ()>(value, &contract, |_, _| panic!()) }.unwrap_err();
        assert!(!error.message().contains("Rust host target"), "{error}");
        if matches!(phase, Rejected::Faulted) {
            assert!(
                error.message().contains("faulted graph publication"),
                "{error}"
            );
        }
    }
}

#[test]
fn scoped_payload_read_restores_loan_after_error_and_panic_and_excludes_reentry() {
    let (runtime, program, root, [contract, _]) = payload();
    let owner = runtime.select_program(&program).unwrap();
    let cx = NativeContext::new(&runtime, &owner).unwrap();
    let value = root.value(runtime.gc()).unwrap();
    let Value::GcHandle(id) = value else {
        panic!("native fixture");
    };
    // SAFETY: The root protects the source. Nested unsafe access is tested only for
    // rejection, and no borrowed references or unretained edges escape.
    unsafe {
        let error = cx
            .read_payload::<u32, ()>(value, &contract, |_, _| {
                assert!(runtime.collect_garbage().is_err());
                assert!(
                    runtime
                        .begin_execution(&owner, runtime.execution_options())
                        .is_err()
                );
                assert!(runtime.gc().take_native::<u32>(id).is_err());
                assert!(
                    cx.read_payload::<u32, ()>(value, &contract, |_, _| panic!())
                        .is_err()
                );
                Err(RuntimeError::module_validation("read closure error"))
            })
            .unwrap_err();
        assert_eq!(
            error.message(),
            RuntimeError::module_validation("read closure error").message()
        );
        assert!(
            catch_unwind(AssertUnwindSafe(|| {
                let _ = cx
                    .read_payload::<u32, ()>(value, &contract, |_, _| panic!("read closure panic"));
            }))
            .is_err()
        );
        assert_eq!(
            cx.read_payload::<u32, _>(value, &contract, |value, _| Ok(*value))
                .unwrap(),
            7
        );
    }
    let alias = ScriptValue::from_root(
        root.clone(),
        contract.argument().unwrap().clone(),
        owner.clone(),
    );
    let borrowed = alias.borrow_payload::<u32>(&cx).unwrap();
    // SAFETY: This guard performs no mutation or recovery. Immediate access must
    // honor another alias's protection instead of bypassing it via a raw value.
    unsafe {
        let guard = borrowed
            .try_enter_operation(&cx, NativeOperationKind::ExclusiveSlots, None)
            .unwrap()
            .unwrap();
        let error = cx
            .read_payload::<u32, ()>(value, &contract, |_, _| panic!())
            .unwrap_err();
        assert!(
            error
                .message()
                .contains("native operation protects receiver slots")
        );
        borrowed
            .read(&cx, Some(&guard), |_, _| {
                let error = borrowed.read::<()>(&cx, None, |_, _| panic!()).unwrap_err();
                assert!(
                    error
                        .message()
                        .contains("native operation protects receiver slots")
                );
                let error = borrowed
                    .read::<()>(&cx, Some(&guard), |_, _| panic!())
                    .unwrap_err();
                assert!(
                    error
                        .message()
                        .contains("script reentry during native storage access")
                );
                Ok(())
            })
            .unwrap();
        drop(guard);
    }
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc().take_native::<u32>(id).unwrap(), 7);
    // SAFETY: Taken identities must reject before the callback.
    assert!(unsafe { cx.read_payload::<u32, ()>(value, &contract, |_, _| panic!()) }.is_err());
}

#[test]
fn scoped_payload_read_polls_cancellation_before_read_and_keeps_mapping_order() {
    let (runtime, program, root, [contract, _]) = payload();
    let owner = runtime.select_program(&program).unwrap();
    let cx = NativeContext::new(&runtime, &owner).unwrap();
    let value = root.value(runtime.gc()).unwrap();
    let options = runtime.execution_options();
    let cancellation = options.cancellation.clone();
    let _session = runtime.begin_execution(&owner, options).unwrap();
    cancellation.cancel();
    // SAFETY: A rooted value; neither failed admission may enter the closure.
    unsafe {
        let error = cx
            .read_payload::<u64, ()>(value, &contract, |_, _| panic!())
            .unwrap_err();
        assert!(error.message().contains("Rust host target"));
        let error = cx
            .read_payload::<u32, ()>(value, &contract, |_, _| panic!())
            .unwrap_err();
        assert_eq!(error.kind(), RuntimeErrorKind::Cancelled);
    }
}

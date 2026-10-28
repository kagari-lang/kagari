use crate::{
    Runtime,
    error::RuntimeErrorKind,
    execution_metadata::MetadataRoot,
    gc::roots::RootedValue,
    module::Program,
    native::{
        binding::NativeBinding,
        builder::ModuleBuilder,
        catalog::DeclarationCatalog,
        conversion::{FromKagari, IntoKagari, context::ConversionContext},
        declarations::FunctionDecl,
        payload::Opaque,
        typed::NativeContext,
        value_handle::ScriptValue,
    },
    test_support::{
        layouts::allocation_owner,
        native::{
            invoke::invoke,
            program::{ProgramFixture, free_function_program},
        },
    },
    value::Value,
};
use kagari_bytecode::{module::BytecodeModule, program::ModuleRef};
use kagari_common::identity::map::DefinitionContext;
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    thread,
};

fn retained_payload() -> (Runtime, Program, ScriptValue, RootedValue) {
    let mut builder =
        ModuleBuilder::new("borrow::payload", &DeclarationCatalog::default()).unwrap();
    let payload = builder
        .define_opaque::<u32>("Payload")
        .unwrap()
        .finish()
        .unwrap();
    let function = builder
        .define_function(FunctionDecl::new("make").returns(payload.apply([]).unwrap()))
        .unwrap();
    builder
        .bind(
            function,
            NativeBinding::new([], payload.codec(), |call| {
                call.allocate_result_payload(&[], |_, _| Ok(7_u32))
            }),
        )
        .unwrap();
    let native = builder.finish().unwrap();
    let mut runtime = Runtime::default();
    native.install(&mut runtime).unwrap();
    let program = runtime
        .load_program("borrow", free_function_program(&native.to_declaration()))
        .unwrap();
    let owner = runtime.select_program(&program).unwrap();
    let value = invoke(&runtime, &owner, 0, &[]).unwrap();
    let root = runtime.root_value(value).unwrap();
    let argument = runtime
        .type_arguments(
            &owner,
            None,
            &[owner.bytecode.native_imports[0].signature.result.clone()],
        )
        .unwrap()
        .pop()
        .unwrap();
    let value = ScriptValue::from_root(root.clone(), argument, owner);
    (runtime, program, value, root)
}

#[test]
fn retained_payload_borrow_preserves_mapping_order_roots_and_alias_checks() {
    let (runtime, program, value, root) = retained_payload();
    let owner = runtime.select_program(&program).unwrap();
    let cx = NativeContext::new(&runtime, &owner).unwrap();
    let roots = runtime.gc().active_roots();
    let borrowed = value.borrow_payload::<u32>(&cx).unwrap();
    assert_eq!(runtime.gc().active_roots(), roots);
    runtime.collect_garbage().unwrap();
    // SAFETY: Only a scalar copy leaves the read; nested admission must reject
    // while the existing payload loan holds collection/reentry exclusion.
    unsafe {
        assert_eq!(
            borrowed
                .read(&cx, None, |payload, _| {
                    let error = value.borrow_payload::<u64>(&cx).err().unwrap();
                    assert!(
                        error
                            .message()
                            .contains("script reentry during native storage access"),
                        "{error}"
                    );
                    Ok(*payload)
                })
                .unwrap(),
            7
        );
    }
    let (foreign, foreign_program, _, _) = retained_payload();
    let foreign_owner = foreign.select_program(&foreign_program).unwrap();
    let foreign_cx = NativeContext::new(&foreign, &foreign_owner).unwrap();
    assert!(value.borrow_payload::<u32>(&foreign_cx).is_err());
    // SAFETY: A foreign receiver must reject without calling the read closure.
    let error = unsafe { borrowed.read::<()>(&foreign_cx, None, |_, _| panic!()) }.unwrap_err();
    assert!(
        error
            .message()
            .contains("foreign or expired provider argument")
    );

    root.set(runtime.gc(), Value::Unit).unwrap();
    let error = value.borrow_payload::<u64>(&cx).err().unwrap();
    assert!(error.message().contains("Rust host target"), "{error}");
    let error = value.borrow_payload::<u32>(&cx).err().unwrap();
    assert!(error.message().contains("native storage root"), "{error}");
    // SAFETY: The failed anchor check runs before granting a payload view.
    let error = unsafe { borrowed.read(&cx, None, |_, _| Ok(())) }.unwrap_err();
    assert!(
        error
            .message()
            .contains("foreign or expired provider argument")
    );

    let mut scalar = value.clone();
    scalar.argument = ConversionContext::new(&runtime, &owner)
        .unwrap()
        .type_for::<i32>()
        .unwrap();
    let error = scalar.borrow_payload::<u32>(&cx).err().unwrap();
    assert!(
        error
            .message()
            .contains("native object requires an opaque storage type")
    );
    assert_eq!(runtime.gc().active_roots(), roots);
}

#[test]
fn retained_payload_read_polls_before_anchor_validation() {
    let (runtime, program, value, root) = retained_payload();
    let owner = runtime.select_program(&program).unwrap();
    let cx = NativeContext::new(&runtime, &owner).unwrap();
    let borrowed = value.borrow_payload::<u32>(&cx).unwrap();
    root.set(runtime.gc(), Value::Unit).unwrap();
    let options = runtime.execution_options();
    let cancellation = options.cancellation.clone();
    let _session = runtime.begin_execution(&owner, options).unwrap();
    cancellation.cancel();
    // SAFETY: Poll must reject before even the now-invalid root is resolved.
    let error = unsafe { borrowed.read::<()>(&cx, None, |_, _| panic!()) }.unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::Cancelled);
}

#[test]
fn retained_payload_borrow_preserves_dynamic_admission_and_mapping_error_order() {
    enum Rejected {
        Closed,
        Faulted,
        Poisoned,
        Condemned,
        Candidate,
    }
    for rejected in [
        Rejected::Closed,
        Rejected::Faulted,
        Rejected::Poisoned,
        Rejected::Condemned,
        Rejected::Candidate,
    ] {
        let (runtime, program, value, root) = retained_payload();
        let owner = runtime.select_program(&program).unwrap();
        let cx = NativeContext::new(&runtime, &owner).unwrap();
        root.set(runtime.gc(), Value::Unit).unwrap();
        if matches!(rejected, Rejected::Candidate) {
            let candidate = runtime
                .stage_reload_verified_program(&program, None, owner.verified_program().clone())
                .unwrap();
            let _initialization = runtime.begin_candidate_initialization(&candidate).unwrap();
            // Borrowing metadata is not a frame call. Keep the public conversion
            // path's phase/mapping precedence rather than imposing call admission.
            let expected = cx
                .conversion_scope(cx.owner())
                .and_then(|conversion| conversion.check_type::<Opaque<u64>>(&value.argument))
                .unwrap_err();
            let error = value.borrow_payload::<u64>(&cx).err().unwrap();
            assert_eq!(error.kind(), expected.kind());
            assert_eq!(error.message(), expected.message());
            continue;
        }
        let expected = match rejected {
            Rejected::Closed => {
                runtime.resources().close_execution();
                runtime.resources().ensure_execution_allowed().unwrap_err()
            }
            Rejected::Faulted => {
                runtime.gc.graph.lock().fault();
                runtime.validate_executable_module(&owner).unwrap();
                runtime.modules().publish_program(&owner).unwrap_err()
            }
            Rejected::Poisoned => {
                assert!(
                    catch_unwind(AssertUnwindSafe(|| {
                        let _marker = runtime.gc.graph.lock();
                        panic!("injected payload view graph poison");
                    }))
                    .is_err()
                );
                runtime.validate_executable_module(&owner).unwrap_err()
            }
            Rejected::Condemned => {
                let mut marker = runtime.gc.graph.lock();
                marker.begin_cycle().unwrap();
                marker.finish_roots().unwrap();
                marker.close_mark().unwrap();
                drop(marker);
                runtime.validate_executable_module(&owner).unwrap_err()
            }
            Rejected::Candidate => unreachable!(),
        };
        let actual = value.borrow_payload::<u64>(&cx).err().unwrap();
        assert_eq!(actual.kind(), expected.kind());
        assert_eq!(actual.message(), expected.message());
    }
}

#[test]
fn scalar_script_values_keep_type_owner_and_cross_thread_lifetime_without_roots() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<ScriptValue>();
    let mut runtime = Runtime::default();
    let owner = allocation_owner(&mut runtime);
    let mut cx = ConversionContext::new(&runtime, &owner).unwrap();
    let expected = cx.type_for::<i32>().unwrap();
    let before = runtime.gc().active_roots();
    assert!(ScriptValue::from_kagari(&mut cx, &expected, &Value::Bool(true)).is_err());
    let value = ScriptValue::from_kagari(&mut cx, &expected, &Value::I32(42)).unwrap();
    assert_eq!(runtime.gc().active_roots(), before);
    assert_eq!(value.value(&runtime).unwrap(), Value::I32(42));
    let wrong = cx.type_for::<bool>().unwrap();
    assert!(value.clone().into_kagari(&mut cx, &wrong).is_err());
    assert_eq!(
        value.clone().into_kagari(&mut cx, &expected).unwrap(),
        Value::I32(42)
    );
    let clone = thread::spawn(move || value.clone()).join().unwrap();
    let mut foreign = Runtime::default();
    let foreign_owner = allocation_owner(&mut foreign);
    let mut foreign_cx = ConversionContext::new(&foreign, &foreign_owner).unwrap();
    let foreign_type = foreign_cx.type_for::<i32>().unwrap();
    assert!(clone.value(&foreign).is_err());
    assert!(
        clone
            .clone()
            .into_kagari(&mut foreign_cx, &foreign_type)
            .is_err()
    );
    drop(cx);
    runtime.collect_garbage().unwrap();
    assert_eq!(clone.value(&runtime).unwrap(), Value::I32(42));
    runtime.resources().close_execution();
    assert_eq!(clone.value(&runtime).unwrap(), Value::I32(42));
    drop(runtime);
    assert!(clone.value(&foreign).is_err());
    thread::spawn(move || drop(clone)).join().unwrap();
}

#[test]
fn from_root_keeps_scalar_alias_mutation_and_attached_program_until_final_drop() {
    let program = || {
        ProgramFixture {
            exports: Default::default(),
            root: ModuleRef::new(0),
            modules: vec![BytecodeModule::default()],
        }
        .metadata(
            DefinitionContext::new().unwrap().snapshot(),
            &Default::default(),
        )
        .unwrap()
    };
    let mut runtime = Runtime::default();
    let loaded = runtime.load_program("retained-root", program()).unwrap();
    let old = runtime.select_program(&loaded).unwrap();
    let expected = ConversionContext::new(&runtime, &old)
        .unwrap()
        .type_for::<i32>()
        .unwrap();
    let root = runtime.root_value(Value::I32(1)).unwrap();
    root.set_metadata(&runtime, vec![MetadataRoot::Program(old.clone())])
        .unwrap();
    let value = ScriptValue::from_root(root.clone(), expected, old.clone());
    let clone = value.clone();
    root.set(runtime.gc(), Value::I32(2)).unwrap();
    assert_eq!(value.value(&runtime).unwrap(), Value::I32(2));
    assert_eq!(clone.value(&runtime).unwrap(), Value::I32(2));
    let candidate = runtime
        .stage_reload_program(&loaded, None, program())
        .unwrap();
    runtime.publish_staged_reload(candidate).unwrap();
    drop(root);
    drop(value);
    assert!(
        runtime
            .collect_garbage()
            .unwrap()
            .reclaimed_modules
            .is_empty()
    );
    assert!(runtime.modules().loaded(old.key()).is_some());
    thread::spawn(move || drop(clone)).join().unwrap();
    assert_eq!(
        runtime.collect_garbage().unwrap().reclaimed_modules,
        [old.key()]
    );
}

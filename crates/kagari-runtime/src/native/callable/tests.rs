use crate::{
    Runtime,
    closure::ClosureMode,
    error::{RuntimeError, RuntimeErrorKind, reason::StaleHandleReason, trap::TrapReason},
    execution::roots::{ExecutionRoots, SessionId},
    executor::native::invoke_script,
    frame::{
        arguments::FrameArguments,
        values::{FrameMetadata, FrameSlots},
    },
    module::{ExecutableModule, dispatch::CallRouteKind, managed::source::RevisionSource},
    native::{
        callable::CallableHandle,
        conversion::{FromKagari, arguments::ArgumentValues, context::ConversionContext},
        function_handle::Function,
        storage::initializer::NativeInit,
        typed::NativeContext,
    },
    test_support::update_programs as programs,
    value::{carriers::ScopedValue, strings::StringObject, view::ValueView},
};
use kagari_bytecode::module::CallableTarget;
use std::marker::PhantomData;

fn fixture() -> (Runtime, ExecutableModule, SessionId, ValueView<'static>) {
    let mut runtime = Runtime::default();
    let installed = runtime
        .load_verified_program(
            "callback",
            programs::source_code(include_str!("../../../tests/fixtures/metadata/closure.kgr")),
        )
        .unwrap();
    let owner = runtime.select_program(&installed).unwrap();
    let function = owner
        .bytecode
        .functions
        .iter()
        .find(|function| {
            function.metadata.params.len() == 1
                && owner
                    .bind_call_route(CallableTarget::Script(function.id))
                    .unwrap()
                    .kind()
                    == CallRouteKind::BodyOwned
        })
        .unwrap()
        .id;
    let execution = runtime.memory.enter_execution(&runtime.resources).unwrap();
    let location = {
        let mut conversion = ConversionContext::for_selected(
            &mut runtime,
            &owner,
            execution,
            RevisionSource::Current,
            None,
        )
        .unwrap();
        let captured = conversion
            .allocate_string("captured callback".into())
            .unwrap();
        let value = conversion
            .allocate_script_closure(function, &[captured], None, ClosureMode::Ordinary)
            .unwrap();
        conversion
            .into_native_value(execution, value)
            .unwrap()
            .location()
            .unwrap()
    };
    let value = runtime.memory.inspect_value(location).unwrap();
    (runtime, owner, execution, value)
}

#[test]
fn stored_provider_and_frame_captures_have_independent_source_lifetimes() {
    let (mut runtime, owner, original, value) = fixture();
    runtime.collect_garbage().unwrap();
    let baseline = runtime.gc_stats();
    let selected = runtime.select_closure_call(original, &value).unwrap();
    let record = selected.record().callable_target();
    let handle = CallableHandle { selected };
    let stored_source = runtime.memory.enter_execution(&runtime.resources).unwrap();
    let stored = runtime
        .memory
        .scope(&runtime.resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                let mut init = NativeInit {
                    slots,
                    _thread: PhantomData,
                };
                let object = handle.store(&mut init)?;
                slots.push_dependency(stored_source, object)
            })
        })
        .unwrap();
    runtime
        .memory
        .retire_execution(&runtime.resources, original)
        .unwrap();
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, baseline.objects + 1);
    assert_eq!(
        runtime.gc_stats().root_registrations,
        baseline.root_registrations
    );
    assert_eq!(
        runtime.resolve_closure(&value).unwrap_err(),
        RuntimeError::from(TrapReason::InvalidClosureHandle)
    );
    // Publish the actual child before leaving its stored provider. A retired
    // original coordinate is never used to recover this still-live object.
    let reopened = runtime
        .memory
        .scope(&runtime.resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                let stored = slots.dependency(stored)?;
                let object = slots
                    .scope()
                    .follow(stored, |stored| Some(&stored.value))
                    .unwrap();
                let value = ScopedValue::managed(object);
                let location = slots.push_local(stored_source, value)?;
                ValueView::protected(value, Some(location))
            })
        })
        .unwrap();
    let frame_source = runtime.memory.enter_execution(&runtime.resources).unwrap();
    let callee = owner.execution().callee(record).unwrap();
    let parameter = callee
        .slot_layout
        .location(callee.parameter_slots[0])
        .unwrap();
    let arguments = FrameArguments::pack(ArgumentValues::scalars(&[]))
        .with_captures(reopened, 1)
        .unwrap();
    let frame = runtime
        .memory
        .scope(&runtime.resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                let body = RevisionSource::Closure(reopened.location().unwrap()).resolve(
                    &runtime.modules,
                    slots,
                    &owner,
                )?;
                Ok(FrameSlots::allocate(
                    slots,
                    frame_source,
                    &callee.slot_layout,
                    &callee.parameter_slots,
                    arguments,
                    FrameMetadata {
                        body,
                        inherited_body: None,
                        dependencies: &[],
                    },
                    &runtime.resources,
                ))
            })
        })
        .unwrap()
        .unwrap();
    runtime
        .memory
        .retire_execution(&runtime.resources, stored_source)
        .unwrap();
    runtime.collect_garbage().unwrap();
    // Both closure and StoredCallable are gone; the frame independently owns the
    // copied String and exact body. Pure handle/record facts do not retain either.
    assert_eq!(runtime.gc_stats().objects, baseline.objects - 1);
    runtime
        .memory
        .scope(&runtime.resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                let value = frame.slots().read(slots, parameter)?;
                let text = slots
                    .scope()
                    .downcast::<StringObject>(value.object().unwrap())?;
                assert_eq!(
                    slots.scope().read(text, |text| text.as_str().to_owned()),
                    "captured callback"
                );
                frame.retire(slots)
            })
        })
        .unwrap();
    runtime
        .memory
        .retire_execution(&runtime.resources, frame_source)
        .unwrap();
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, baseline.objects - 2);
    assert_eq!(
        runtime.gc_stats().root_registrations,
        baseline.root_registrations
    );
    assert!(!runtime.is_quarantined());
}

#[test]
fn rooted_callback_retains_captures_but_not_storage_after_runtime_teardown() {
    let (mut runtime, owner, original, value) = fixture();
    let record = runtime.resolve_closure(&value).unwrap();
    let lifetime = runtime.resources().lifetime_probe();
    let rooted = {
        let mut conversion = ConversionContext::new(&mut runtime, &owner).unwrap();
        let expected = conversion.type_for::<Function<(), String>>().unwrap();
        let rooted =
            Function::<(), String>::from_kagari(&mut conversion, &expected, &value).unwrap();
        // A pure record is not a borrowed payload. Existing source values remain
        // readable while normal conversion mutation and collection proceed.
        let element = conversion.type_for::<i32>().unwrap();
        let array = conversion.allocate_array(&element, &[1i32.into()]).unwrap();
        conversion.array_set(&array, 0usize, &2i32.into()).unwrap();
        assert_eq!(
            conversion
                .array_element(&array, 0usize)
                .unwrap()
                .unwrap()
                .as_i32(),
            Some(2)
        );
        conversion.runtime_mut().collect_garbage().unwrap();
        assert_eq!(record.signature().params.len(), 0);
        rooted
    };
    runtime
        .memory
        .retire_execution(&runtime.resources, original)
        .unwrap();
    runtime.collect_garbage().unwrap();
    drop(runtime);
    assert!(lifetime.upgrade().is_none());
    let (mut foreign, foreign_owner, _, _) = fixture();
    let mut cx = NativeContext::with_invoker(&mut foreign, &foreign_owner, invoke_script).unwrap();
    let error = rooted.call(&mut cx, ()).unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::StaleHandle);
    assert_eq!(
        error.to_string(),
        RuntimeError::from(StaleHandleReason::ExecutableModuleBelongsToAnotherRuntime).to_string()
    );
}

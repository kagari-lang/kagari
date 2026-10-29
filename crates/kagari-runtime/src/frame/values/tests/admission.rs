//! Argument source validation and late bank exclusion.
use super::{TestFrameMetadata, module};
use crate::{
    error::{RuntimeError, RuntimeErrorKind},
    frame::{FrameDispatch, FrameEntry, arguments::FrameArguments, types::EnvironmentRecord},
    gc::graph::MarkPhase,
    module::execution::{OperandSlot, layout::Location},
    native::storage::NativeStorage,
    value::Value,
};
use kagari_abi::representation::ValueType;
use kagari_bytecode::module::CallableTarget;
use kagari_types::{scalar::BuiltinType, ty::Ty};
use std::panic::{AssertUnwindSafe, catch_unwind};

#[test]
fn rooted_arguments_are_revalidated_after_source_mutation_and_native_take() {
    let (runtime, _program, owner) = module();
    let heap = runtime.gc();
    let object = NativeStorage::takeable::<i32>()
        .prepare_payload(heap, &Ty::Builtin(BuiltinType::I32), 17, &owner)
        .unwrap();
    let id = heap.alloc_native(object).unwrap();
    let value = Value::GcHandle(id);
    let source = runtime
        .resources()
        .frame_values
        .borrow_mut()
        .allocate_test(
            &runtime,
            1,
            &[0],
            &FrameArguments::plain(&[value]),
            TestFrameMetadata {
                program: owner.clone(),
                environment: None,
                invocation: None,
            },
            None,
        )
        .unwrap();
    let locations = [Location {
        operand: OperandSlot::new(0, true),
        representation: ValueType::Generic,
        semantic: None,
        borrowed_receiver: false,
    }];
    let arguments = FrameArguments::frame(source, &locations);
    assert!(
        arguments
            .all_managed(&runtime, |argument| {
                assert!(source.set(heap, 0, Value::I32(9)).is_none());
                *argument == value && heap.validate_value(argument)
            })
            .unwrap()
    );
    source.set(heap, 0, Value::I32(9)).unwrap();
    assert!(
        arguments
            .all_managed(&runtime, |argument| *argument == Value::I32(9))
            .unwrap()
    );
    source.set(heap, 0, value).unwrap();
    assert_eq!(heap.take_native::<i32>(id).unwrap(), 17);
    // The window still roots the handle, but take made it inert. Neither the
    // admission check nor publication may treat the earlier validation as proof.
    assert!(
        !arguments
            .all_managed(&runtime, |argument| heap.validate_value(argument))
            .unwrap()
    );
    let mut values = runtime.resources().frame_values.borrow_mut();
    let generation = values.next_generation;
    let error = values
        .allocate_test(
            &runtime,
            1,
            &[0],
            &arguments,
            TestFrameMetadata {
                program: owner,
                environment: None,
                invocation: None,
            },
            None,
        )
        .unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::ModuleValidation);
    assert_eq!(values.next_generation, generation);
    assert_eq!(values.active_windows(), 1);
    assert_eq!(values.get(source).unwrap(), &[value]);
    values.release(source).unwrap();
}

#[test]
fn early_argument_shading_is_retained_on_success_and_later_arity_failure() {
    for accepted in [false, true] {
        let (runtime, _program, owner) = super::publication::source_module(
            "pub fn hold(value: (i32, i32)) -> (i32, i32) { value }",
        );
        let entry = owner.bytecode.functions[0].id;
        let child = runtime
            .gc
            .alloc_tuple(vec![Value::I32(1), Value::I32(2)])
            .unwrap();
        let stack = runtime.enter_execution_stack(&owner).unwrap();
        runtime.gc.graph.lock().begin_cycle().unwrap();
        runtime.gc.graph.lock().finish_roots().unwrap();
        let arguments = [child, child];
        let result = stack.push_admitted_arguments(
            &runtime,
            owner.clone(),
            CallableTarget::Script(entry),
            FrameArguments::plain(&arguments[..if accepted { 1 } else { 2 }]),
            None,
            FrameDispatch {
                prepared: None,
                entry: FrameEntry::Call,
                invocation: None,
                environment: None,
            },
        );
        if accepted {
            result.unwrap();
            assert_eq!(
                stack.current().unwrap().slots.get(runtime.gc(), 0),
                Some(child)
            );
        } else {
            assert!(
                result
                    .unwrap_err()
                    .message()
                    .contains("frame argument count")
            );
            assert_eq!(
                runtime.resources().frame_values.borrow().active_windows(),
                0
            );
        }
        assert_eq!(
            runtime.resources().counters().current_call_depth,
            u32::from(accepted)
        );
        runtime.gc.drain_graph_work(&runtime.modules).unwrap();
        runtime.gc.graph.lock().close_mark().unwrap();
        assert!(
            runtime
                .gc
                .graph_survivor(&runtime.modules, child.heap_identity().unwrap().graph(),)
                .unwrap()
        );
        runtime.gc.graph.lock().finish_sweep().unwrap();
        drop(stack);
        assert_eq!(runtime.resources().counters().current_call_depth, 0);
    }
}

#[test]
fn deferred_graph_failure_preserves_later_heap_and_precommit_error_precedence() {
    for failure in [
        "arity",
        "heap",
        "environment",
        "transfer",
        "reservation",
        "publication",
    ] {
        let (runtime, _program, owner) = super::publication::source_module(
            "pub fn hold(a: (i32, i32), b: (i32, i32)) -> (i32, i32) { a }",
        );
        let entry = owner.bytecode.functions[0].id;
        let child = runtime
            .gc
            .alloc_tuple(vec![Value::I32(1), Value::I32(2)])
            .unwrap();
        let foreign = crate::Runtime::default();
        let foreign_value = foreign
            .gc
            .alloc_tuple(vec![Value::I32(3), Value::I32(4)])
            .unwrap();
        let environment = (failure == "environment").then(|| {
            foreign
                .alloc_environment(
                    EnvironmentRecord::new(foreign.definition_context(), vec![], vec![]).unwrap(),
                )
                .unwrap()
        });
        let stack = runtime.enter_execution_stack(&owner).unwrap();
        // The module survives Sweep but the argument is condemned. Identity
        // validation still succeeds; publication fails before preparation starts.
        runtime.gc.graph.lock().begin_cycle().unwrap();
        runtime.publish_executable_module(&owner).unwrap();
        runtime.gc.graph.lock().finish_roots().unwrap();
        runtime.gc.drain_graph_work(&runtime.modules).unwrap();
        runtime.gc.graph.lock().close_mark().unwrap();
        assert!(runtime.gc.validate_value(&child));
        assert_eq!(
            runtime.gc.publish_value(child).unwrap_err().message(),
            RuntimeError::module_validation("condemned graph edge").message()
        );
        if failure == "reservation" {
            runtime
                .resources()
                .frame_values
                .borrow_mut()
                .next_generation = u64::MAX;
        }
        let generation = runtime.resources().frame_values.borrow().next_generation;
        let second = match failure {
            "heap" => foreign_value,
            "transfer" => Value::I32(9),
            _ => child,
        };
        let arguments = [child, second, child];
        let error = stack
            .push_admitted_arguments(
                &runtime,
                owner.clone(),
                CallableTarget::Script(entry),
                FrameArguments::plain(&arguments[..if failure == "arity" { 3 } else { 2 }]),
                None,
                FrameDispatch {
                    prepared: None,
                    entry: FrameEntry::Call,
                    invocation: None,
                    environment,
                },
            )
            .unwrap_err();
        let expected = match failure {
            "arity" => "frame argument count does not match the linked function",
            "heap" => "invalid heap argument",
            "environment" => "invalid execution environment",
            "transfer" => "invalid frame argument type or window",
            "reservation" => "frame window generations",
            "publication" => "condemned graph edge",
            _ => unreachable!(),
        };
        let expected = if failure == "reservation" {
            RuntimeError::resource_limit(expected)
        } else {
            RuntimeError::module_validation(expected)
        };
        assert_eq!(error.kind(), expected.kind(), "{failure}");
        assert_eq!(error.message(), expected.message(), "{failure}");
        assert_eq!(runtime.resources().counters().current_call_depth, 0);
        let values = runtime.resources().frame_values.borrow();
        assert_eq!(values.active_windows(), 0);
        assert_eq!(values.next_generation, generation);
        assert!(values.values.is_empty());
        assert!(values.payloads.is_empty());
        assert!(values.initialized.is_empty());
        drop(values);
        runtime.gc.graph.lock().finish_sweep().unwrap();
    }
}

#[test]
fn early_publication_does_not_hide_fault_or_poison_metadata_rejection() {
    for poisoned in [false, true] {
        let (runtime, _program, owner) = super::publication::source_module(
            "pub fn hold(value: (i32, i32)) -> (i32, i32) { value }",
        );
        let entry = owner.bytecode.functions[0].id;
        let child = runtime
            .gc
            .alloc_tuple(vec![Value::I32(1), Value::I32(2)])
            .unwrap();
        let stack = runtime.enter_execution_stack(&owner).unwrap();
        if poisoned {
            assert!(
                catch_unwind(AssertUnwindSafe(|| {
                    let _guard = runtime.gc.graph.lock();
                    panic!("injected frame publication poison");
                }))
                .is_err()
            );
        } else {
            runtime.gc.graph.lock().fault();
            assert_eq!(runtime.gc.graph.lock().phase(), MarkPhase::Faulted);
        }
        assert!(runtime.gc.validate_value(&child));
        let error = stack
            .push_admitted_arguments(
                &runtime,
                owner.clone(),
                CallableTarget::Script(entry),
                FrameArguments::plain(&[child]),
                None,
                FrameDispatch {
                    prepared: None,
                    entry: FrameEntry::Call,
                    invocation: None,
                    environment: None,
                },
            )
            .unwrap_err();
        if poisoned {
            assert_eq!(error.kind(), RuntimeErrorKind::StaleHandle);
            assert_eq!(error.message(), "executable module has been released");
        } else {
            assert_eq!(error.kind(), RuntimeErrorKind::ModuleValidation);
            assert_eq!(
                error.message(),
                RuntimeError::module_validation("faulted graph publication").message()
            );
        }
        assert_eq!(runtime.resources().counters().current_call_depth, 0);
        assert_eq!(
            runtime.resources().frame_values.borrow().active_windows(),
            0
        );
        assert_eq!(runtime.resources().frame_values.borrow().next_generation, 0);
        drop(stack);
        assert!(!runtime.is_quarantined());
    }
}

#[test]
fn shared_source_readers_preserve_arity_before_late_exclusive_borrow_failure() {
    let (runtime, _program, owner) =
        super::publication::source_module("pub fn hold(value: (i32, i32)) -> (i32, i32) { value }");
    let entry = owner.bytecode.functions[0].id;
    let value = runtime
        .gc
        .alloc_tuple(vec![Value::I32(1), Value::I32(2)])
        .unwrap();
    let stack = runtime.enter_execution_stack(&owner).unwrap();
    stack
        .push(&runtime, owner.slot(), entry, &[value], None)
        .unwrap();
    let source = stack.current().unwrap().slots;
    let location = owner.execution().functions[entry.index()]
        .frame
        .slot_layout
        .locations[0];
    let external_reader = runtime.resources().frame_values.borrow();
    for locations in [&[][..], &[location][..]] {
        let error = stack
            .push_admitted_arguments(
                &runtime,
                owner.clone(),
                CallableTarget::Script(entry),
                FrameArguments::frame(source, locations),
                None,
                FrameDispatch {
                    prepared: None,
                    entry: FrameEntry::Call,
                    invocation: None,
                    environment: None,
                },
            )
            .unwrap_err();
        if locations.is_empty() {
            assert_eq!(error.kind(), RuntimeErrorKind::ModuleValidation);
            assert!(error.message().contains("frame argument count"));
            assert!(!runtime.is_quarantined());
        } else {
            assert_eq!(error.kind(), RuntimeErrorKind::EngineFault);
            assert!(
                error
                    .message()
                    .contains("execution slots borrowed during frame entry")
            );
        }
        assert_eq!(external_reader.active_windows(), 1);
        assert_eq!(runtime.resources().counters().current_call_depth, 1);
        assert_eq!(
            external_reader.with_value(source, 0, |value| *value),
            Some(value)
        );
    }
    drop(external_reader);
    assert!(runtime.resources().frame_values.try_borrow_mut().is_ok());
    drop(stack);
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
    assert_eq!(
        runtime.resources().frame_values.borrow().active_windows(),
        0
    );
}

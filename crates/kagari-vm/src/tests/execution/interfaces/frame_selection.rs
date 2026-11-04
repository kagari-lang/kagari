use super::*;
use kagari_runtime::frame::arguments::InvocationArguments;

#[test]
fn interface_frames_and_descendants_select_current_code() {
    let source = "trait Tag { fn tag(self) -> i32; } impl Tag for i32 { fn tag(self) -> i32 { helper() + self } } fn helper() -> i32 { 1 } fn main() -> i32 { 0 }";
    let old_code = compile_test_bytecode(source);
    let new_code = compile_test_bytecode(
        &source.replace("fn helper() -> i32 { 1 }", "fn helper() -> i32 { 2 }"),
    );
    let mut runtime = standard_runtime(Default::default());
    let old = runtime.load_program("interface-frames", old_code).unwrap();
    let _old_program = old;
    let old = runtime.select_program(&_old_program).unwrap();
    let method = old.bytecode.interface_tables[0].methods[0].method;
    let boxed = runtime.make_interface(&old, 0, Value::I32(7)).unwrap();
    let receiver_root = runtime.root_value(boxed).unwrap();
    let candidate = runtime
        .stage_reload_program(&_old_program, Some("interface-frames".into()), new_code)
        .unwrap();
    let new = candidate.module().clone();
    runtime.publish_staged_reload(candidate).unwrap();
    let _new_program = _old_program.clone();
    let resolved = runtime.resolve_interface_method(&boxed, &method).unwrap();
    let entry = new
        .bytecode
        .functions
        .iter()
        .find(|f| f.name == "main")
        .unwrap()
        .id;
    let helper = old
        .bytecode
        .functions
        .iter()
        .find(|f| f.name == "helper")
        .unwrap()
        .id;
    let stack = runtime.enter_execution_stack(&new).unwrap();
    stack.push(&runtime, new.slot(), entry, &[], None).unwrap();
    let wrong = runtime.resolve_interface_method(&boxed, &method).unwrap();
    assert!(
        stack
            .push_interface_method(
                &runtime,
                wrong,
                InvocationArguments::from_values(&[Value::Bool(true)]),
                None
            )
            .is_err()
    );
    assert_eq!(stack.current().unwrap().loaded().key(), new.key());
    stack
        .push_interface_method(
            &runtime,
            resolved,
            InvocationArguments::from_values(&[Value::I32(7)]),
            None,
        )
        .unwrap();
    // The callee must retain selection metadata after its host lease and the
    // receiver's external root are gone, even while a newer program is current.
    drop(receiver_root);
    runtime.collect_garbage().unwrap();
    assert_eq!(stack.current().unwrap().loaded().key(), new.key());
    stack.push(&runtime, new.slot(), helper, &[], None).unwrap();
    assert_eq!(stack.current().unwrap().loaded().key(), new.key());
    stack.pop().unwrap();
    assert_eq!(
        stack
            .finish_return(&runtime, ReturnValue::general(Value::I32(8)))
            .unwrap(),
        ReturnAction::Continue(ExecutionAction::Script)
    );
    assert_eq!(stack.current().unwrap().loaded().key(), new.key());
    stack.pop().unwrap();
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
}

use kagari_bytecode::{
    module::BytecodeModule,
    program::{BytecodeProgram, ModuleRef},
};
use kagari_common::cancellation::CancellationToken;
use kagari_common::host_interface::{HostFunctionDeclaration, value_type::HostValueType};
use kagari_runtime::{Runtime, host::HostFunction, value::Value};
use kagari_runtime::{error::RuntimeErrorKind, session::ExecutionOptions};

#[test]
fn installed_function_needs_no_execution_permissions() {
    let mut runtime = Runtime::default();
    assert!(runtime.invoke_host("host.answer", &[]).is_err());
    runtime
        .register_host_function(HostFunction::new(
            HostFunctionDeclaration::new("host.answer", vec![], HostValueType::I32),
            |_, _| Ok(Value::I32(42)),
        ))
        .unwrap();
    assert_eq!(
        runtime.invoke_host("host.answer", &[]).unwrap(),
        Value::I32(42)
    );
}

#[test]
fn host_cancellation_is_sticky_even_when_the_handler_returns_success() {
    let mut runtime = Runtime::default();
    let cancel = CancellationToken::default();
    let callback_cancel = cancel.clone();
    runtime
        .register_host_function(HostFunction::new(
            HostFunctionDeclaration::new("host.cancel", vec![], HostValueType::I32),
            move |_, _| {
                callback_cancel.cancel();
                Ok(Value::I32(42))
            },
        ))
        .unwrap();
    let module = runtime
        .load_program(
            "cancel",
            BytecodeProgram {
                root: ModuleRef::new(0),
                modules: vec![BytecodeModule::default()],
            },
        )
        .unwrap();
    let session = runtime
        .begin_execution(
            &module,
            ExecutionOptions {
                cancellation: cancel,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(
        runtime.invoke_host("host.cancel", &[]).unwrap_err().kind(),
        RuntimeErrorKind::Cancelled
    );
    assert_eq!(
        runtime.resources().poll_execution().unwrap_err().kind(),
        RuntimeErrorKind::Cancelled
    );
    drop(session);
    let _next = runtime
        .begin_execution(&module, ExecutionOptions::default())
        .unwrap();
    runtime.resources().poll_execution().unwrap();
}

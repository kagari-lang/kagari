use kagari_common::host_interface::{HostFunctionDeclaration, value_type::HostValueType};
use kagari_runtime::{Runtime, host::HostFunction, value::Value};

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

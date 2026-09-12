use kagari_common::host_interface::{
    HostFunctionDeclaration, HostInterface, HostParameter, HostPassingStyle, HostValueType,
};
use kagari_runtime::{
    CapabilitySet, HostExposurePolicy, LanguageProfile, Runtime, RuntimeConfig, SecurityContext,
    host::{HostError, HostFunction},
    value::Value,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // This definition can be exported by a separate tool with no runtime or services.
    let mut declaration = HostFunctionDeclaration::new(
        "demo.echo",
        vec![HostParameter {
            name: "value".into(),
            ty: HostValueType::I32,
            passing: HostPassingStyle::Owned,
        }],
        HostValueType::I32,
    );
    declaration.effects.may_trap = true;
    declaration.documentation = "Return the supplied integer.".into();
    let bytes = HostInterface {
        functions: vec![declaration.clone()],
    }
    .to_bytes()?;
    let expected = HostInterface::from_bytes(&bytes)?;

    let mut runtime = Runtime::new(RuntimeConfig {
        security: SecurityContext {
            profile: LanguageProfile {
                allow_host_calls: true,
                ..Default::default()
            },
            capabilities: CapabilitySet {
                host_calls: true,
                ..Default::default()
            },
        },
        host_exposure: HostExposurePolicy {
            allowed_host_functions: vec!["demo.echo".into()],
            ..Default::default()
        },
        ..Default::default()
    });
    runtime.register_host_function(HostFunction::new(declaration, |args| match args {
        [Value::I32(value)] => Ok(Value::I32(*value)),
        _ => Err(HostError::new("echo expects one i32")),
    }))?;
    let loaded = runtime.load_program(
        "offline-demo",
        kagari_ir::bytecode::BytecodeProgram {
            root: kagari_ir::bytecode::ModuleRef::new(0),
            modules: vec![kagari_ir::bytecode::BytecodeModule {
                host_interface: expected,
                ..Default::default()
            }],
        },
    )?;
    let binding = loaded
        .host_binding(kagari_ir::bytecode::HostImportId::new(0))
        .unwrap();
    assert_eq!(
        runtime.invoke_bound_host(binding, &[Value::I32(42)])?,
        Value::I32(42)
    );
    println!(
        "offline interface: {} bytes; linked echo returned 42",
        bytes.len()
    );
    Ok(())
}

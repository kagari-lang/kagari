use kagari_common::host_interface::{HostFunctionDeclaration, HostInterface, HostValueType};
use kagari_runtime::{
    CapabilitySet, HostExposurePolicy, LanguageProfile, Runtime, RuntimeConfig, SecurityContext,
    host::HostFunction, value::Value,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // This definition can be exported by a separate tool with no runtime or services.
    let mut declaration = HostFunctionDeclaration::new("demo.limit", vec![], HostValueType::I32);
    declaration.effects.may_read_immutable_configuration = true;
    declaration.documentation = "Read the immutable request limit snapshot.".into();
    let bytes = HostInterface {
        field_paths: vec![],
        types: Vec::new(),
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
            allowed_host_functions: vec!["demo.limit".into()],
            ..Default::default()
        },
        ..Default::default()
    });
    let immutable_limit = 42;
    runtime.register_host_function(HostFunction::new(declaration, move |_, _| {
        Ok(Value::I32(immutable_limit))
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
    let mut options = runtime.execution_options();
    options.record_host_calls = true;
    options.inputs.unix_time_millis = 1_000;
    let session = runtime.begin_execution(&loaded, options)?;
    assert_eq!(runtime.invoke_bound_host(binding, &[])?, Value::I32(42));
    let trace = session.trace().expect("recording was enabled");
    assert_eq!(trace.code_fingerprint, loaded.program_fingerprint());
    assert_eq!(trace.inputs.unix_time_millis, 1_000);
    assert_eq!(trace.host_calls.len(), 1);
    assert_eq!(trace.host_calls[0].symbol, "demo.limit");
    assert_eq!(
        trace.host_calls[0].outcome,
        Some(Ok(kagari_runtime::TraceValue::I32(42)))
    );
    drop(session);
    let candidate = runtime.stage_reload_program(
        &loaded,
        "offline-demo",
        kagari_ir::bytecode::BytecodeProgram {
            root: kagari_ir::bytecode::ModuleRef::new(0),
            modules: vec![(*loaded.bytecode).clone()],
        },
    )?;
    let initialization = runtime.begin_candidate_initialization(&candidate)?;
    assert_eq!(runtime.invoke_bound_host(binding, &[])?, Value::I32(42));
    drop(initialization);
    runtime.publish_staged_reload(candidate)?;
    println!(
        "offline interface: {} bytes; immutable configuration returned 42",
        bytes.len()
    );
    Ok(())
}

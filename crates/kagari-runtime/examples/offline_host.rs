use kagari_bytecode::{
    module::BytecodeModule,
    program::{BytecodeProgram, ModuleRef},
};
use kagari_common::identity::{map::DefinitionContext, metadata::DefinitionMetadata};
use kagari_runtime::{
    Runtime, RuntimeConfig, host::HostFunction, session::TraceValue, value::Value,
};
use kagari_types::host_interface::{
    HostFunctionDeclaration, HostInterface, value_type::HostValueType,
};
use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    // This definition can be exported by a separate tool with no runtime or services.
    let context = DefinitionContext::new()?;
    let mut declaration =
        HostFunctionDeclaration::new(&context, "demo.limit", vec![], HostValueType::I32)?;
    declaration.effects.may_read_immutable_configuration = true;
    declaration.documentation = "Read the immutable request limit snapshot.".into();
    let bytes = HostInterface {
        paths: vec![],
        types: Vec::new(),
        functions: vec![declaration.clone()],
    }
    .to_bytes(&context.snapshot())?;
    let expected = HostInterface::from_bytes(&bytes)?;

    let mut runtime = Runtime::new(RuntimeConfig {
        ..Default::default()
    });
    let immutable_limit = 42;
    let declaration =
        DefinitionMetadata::checked(context.snapshot(), declaration, &Default::default())?;
    let binding = runtime.register_host_function(HostFunction::new(declaration, move |_, _| {
        Ok(Value::I32(immutable_limit))
    }))?;
    // Decoding supplies a new identity scope. Retain it through program validation.
    let (definitions, expected) = expected.into_parts();
    let loaded = runtime.load_program(
        "offline-demo",
        DefinitionMetadata::checked(
            definitions,
            BytecodeProgram {
                root: ModuleRef::new(0),
                modules: vec![BytecodeModule {
                    host_interface: expected,
                    ..Default::default()
                }],
            },
            &Default::default(),
        )?,
    )?;
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
    assert_eq!(trace.host_calls[0].outcome, Some(Ok(TraceValue::I32(42))));
    drop(session);
    let (definitions, module) = loaded.to_unverified(&Default::default())?.into_parts();
    let candidate = runtime.stage_reload_program(
        &loaded,
        "offline-demo",
        DefinitionMetadata::checked(
            definitions,
            BytecodeProgram {
                root: ModuleRef::new(0),
                modules: vec![module],
            },
            &Default::default(),
        )?,
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

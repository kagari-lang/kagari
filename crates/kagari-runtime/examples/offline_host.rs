//! Cold common declarations link before execution and retain session/reload behavior.
//! Run with `cargo run -p kagari-runtime --example offline_host`.
use kagari_bytecode::{
    artifact::{ArtifactBuildOptions, KbcArtifact},
    instruction::NativeImportId,
    module::{BytecodeModule, CallableTarget},
    program::{BytecodeProgram, ModuleRef},
};
use kagari_common::identity::{
    DefinitionKind, ModuleIdentity, PackageId, map::DefinitionContext, metadata::DefinitionMetadata,
};
use kagari_contract::{
    native_import::NativeImport,
    types::{ConcreteFunctionIdentity, PublicItem},
};
use kagari_runtime::{
    Runtime,
    frame::driver::ReturnAction,
    host::HostCallContext,
    module::LoadedModule,
    native::{binding::NativeResult, builder::ModuleBuilder, catalog::DeclarationCatalog},
    session::TraceValue,
    value::Value,
};
use kagari_types::{
    callable::{CallableImplementation, MethodPolicy, Signature},
    declaration::{
        FnDecl,
        host::{HostCallPolicy, HostFunctionEffects},
        module::ModuleDecl,
    },
    scalar::BuiltinType,
    ty::Ty,
};
use std::{collections::BTreeMap, error::Error};

fn cold_declarations() -> Result<DefinitionMetadata<ModuleDecl>, Box<dyn Error>> {
    let context = DefinitionContext::new()?;
    let identity = ModuleIdentity {
        package: PackageId("demo".into()),
        path: vec![],
    };
    let root = context.intern_root(&identity)?;
    let limit = context.intern_child(root, DefinitionKind::Function, "limit", 0)?;
    let mut declarations = ModuleDecl::new(identity);
    declarations.functions.push(FnDecl {
        name: "limit".into(),
        implementation: CallableImplementation::Native(limit),
        host_policy: Some(HostCallPolicy {
            passing: vec![],
            effects: HostFunctionEffects {
                may_read_immutable_configuration: true,
                ..Default::default()
            },
        }),
        method_policy: MethodPolicy::default(),
        generic_params: vec![],
        bounds: vec![],
        params: vec![],
        return_type: Ty::Builtin(BuiltinType::I32),
    });
    declarations
        .documentation
        .insert(limit, "Read the immutable request limit snapshot.".into());
    declarations.collect_definitions(&context)?;
    Ok(DefinitionMetadata::checked(
        context.snapshot(),
        declarations,
        &Default::default(),
    )?)
}

fn invoke_limit(runtime: &Runtime, owner: &LoadedModule) -> NativeResult<Value> {
    let stack = runtime.enter_execution_stack(owner)?;
    stack.push_callable(
        runtime,
        owner.clone(),
        CallableTarget::Native(NativeImportId::new(0)),
        &[],
        None,
    )?;
    stack.start_native_entry(runtime, |_, _, _, _| {
        unreachable!("limit never calls script")
    })?;
    let ReturnAction::Complete(value) = stack.finish_native_return(runtime)? else {
        panic!("root native return");
    };
    Ok(value)
}

fn main() -> Result<(), Box<dyn Error>> {
    // A separate tool can author and encode this contract without Rust implementations.
    let cold = cold_declarations()?;
    let definitions = DefinitionContext::new()?;
    let declaration = cold.import_into(&definitions, &Default::default())?;
    let records = declaration.records();
    let required = records.native_declarations(declaration.definitions())?;
    let limit = &required[0];
    let CallableImplementation::Native(binding) = limit.function.implementation else {
        unreachable!("authored native entry")
    };
    let import = NativeImport {
        receiver_access: None,
        result_adapter: None,
        generic: None,
        instance: ConcreteFunctionIdentity {
            declaration: limit.declaration,
            arguments: vec![],
        },
        binding,
        signature: Signature {
            params: vec![],
            result: limit.function.return_type.clone(),
        },
        requirements: vec![],
        callables: vec![],
    };
    let artifact = KbcArtifact::from_program(
        BytecodeProgram {
            exports: Default::default(),
            root: ModuleRef::new(0),
            modules: vec![BytecodeModule {
                identity: records.identity.clone(),
                source_name: "offline-demo".into(),
                public_items: vec![PublicItem::Function(limit.function.clone())],
                native_declarations: required,
                native_imports: vec![import],
                ..Default::default()
            }],
        },
        ArtifactBuildOptions::default(),
        &definitions,
    )?;
    let bytes = artifact.to_bytes(&definitions.snapshot())?;
    let decoded = KbcArtifact::from_bytes(&bytes)?;

    let mut builder =
        ModuleBuilder::from_declaration(cold, &DeclarationCatalog::default(), BTreeMap::new())?;
    let function = builder.declared_function("limit")?;
    let immutable_limit = 42;
    builder.bind(function, move |_: &HostCallContext<'_>, _: &[Value]| {
        Ok(Value::I32(immutable_limit))
    })?;
    let installed = builder.finish()?;
    let mut runtime = Runtime::default();
    installed.install(&mut runtime)?;
    let decoded = decoded.import_into(&definitions, &Default::default())?;
    decoded
        .records()
        .validate_for_loader(&Default::default(), &definitions)?;
    let (snapshot, decoded) = decoded.into_parts();
    let loaded = runtime.load_program(
        "offline-demo",
        DefinitionMetadata::checked(snapshot, decoded.program, &Default::default())?,
    )?;
    let mut options = runtime.execution_options();
    options.record_host_calls = true;
    options.inputs.unix_time_millis = 1_000;
    let session = runtime.begin_execution(&loaded, options)?;
    assert_eq!(invoke_limit(&runtime, &loaded)?, Value::I32(42));
    let trace = session.trace().expect("recording was enabled");
    assert_eq!(trace.code_fingerprint, loaded.program_fingerprint());
    assert_eq!(trace.inputs.unix_time_millis, 1_000);
    assert_eq!(trace.host_calls.len(), 1);
    assert_eq!(trace.host_calls[0].symbol, "demo::limit");
    assert_eq!(trace.host_calls[0].outcome, Some(Ok(TraceValue::I32(42))));
    drop(session);
    let (definitions, module) = loaded.to_unverified(&Default::default())?.into_parts();
    let candidate = runtime.stage_reload_program(
        &loaded,
        "offline-demo",
        DefinitionMetadata::checked(
            definitions,
            BytecodeProgram {
                exports: Default::default(),
                root: ModuleRef::new(0),
                modules: vec![module],
            },
            &Default::default(),
        )?,
    )?;
    let initialization = runtime.begin_candidate_initialization(&candidate)?;
    assert_eq!(invoke_limit(&runtime, candidate.module())?, Value::I32(42));
    drop(initialization);
    runtime.publish_staged_reload(candidate)?;
    assert_eq!(runtime.gc().active_roots(), 0);
    println!(
        "common offline contract: {} bytes; immutable configuration returned 42",
        bytes.len()
    );
    Ok(())
}

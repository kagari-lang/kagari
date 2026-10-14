//! Offline host associated-output declarations and dynamic interface binding.
//! Run with `cargo run -p kagari-embed --example host_interfaces`.

use {
    kagari_common::identity::{
        DefinitionKind, DefinitionPath, DefinitionPathSegment, map::DefinitionContext,
        metadata::DefinitionMetadata,
    },
    kagari_source::source_database::SourceLayer,
    kagari_types::host_interface::{
        HostFunctionDeclaration, HostInterface, HostParameter, HostPassingStyle,
        type_declaration::{
            HostAssociatedTypeBinding, HostMethodDeclaration, HostTraitImplementationDeclaration,
            HostTraitMethodBinding, HostTypeDeclaration, HostTypeOwnership, PathAccess,
        },
        value_type::HostValueType,
    },
};

use kagari_embed::{
    BytecodeArtifact,
    context::{ExecutionContext, JitPolicy},
    engine::KagariEngine,
    program::PreparedProgram,
};
use kagari_runtime::{
    host::{HostError, HostFunction, HostObjectId, HostSchemaEpoch, HostTypeRegistration},
    module::entry::ProgramEntry,
    value::Value,
};

const SOURCE: &str = include_str!("../../../examples/host-interfaces.kgr");

fn compile_offline() -> (
    KagariEngine,
    DefinitionMetadata<BytecodeArtifact>,
    DefinitionMetadata<HostTypeDeclaration>,
    DefinitionMetadata<HostFunctionDeclaration>,
) {
    let definitions = DefinitionContext::new().unwrap();
    let engine = KagariEngine::default();
    let file = engine
        .set_source("mem://host-interface", SOURCE.into(), SourceLayer::Base)
        .unwrap();
    let trait_id = DefinitionPath {
        module: engine
            .source_snapshot()
            .file(file)
            .unwrap()
            .module_identity()
            .clone(),
        path: vec![DefinitionPathSegment {
            kind: DefinitionKind::Trait,
            name: "Reader".into(),
            occurrence: 0,
        }],
    };
    let trait_id = definitions.intern(&trait_id).unwrap();
    let mut host = HostTypeDeclaration::new(&definitions, "demo.Counter").unwrap();
    host.ownership = HostTypeOwnership::HostRoot;
    host.path_access = PathAccess::ReadOnly;
    let method = HostMethodDeclaration::new(
        &definitions,
        host.id,
        "read",
        vec![HostParameter {
            name: "amount".into(),
            ty: HostValueType::I32,
            passing: HostPassingStyle::Owned,
        }],
        HostValueType::I32,
    )
    .unwrap();
    let mut implementation = HostTraitImplementationDeclaration::new(
        trait_id,
        vec![],
        vec![HostTraitMethodBinding {
            trait_method: definitions
                .intern_child(trait_id, DefinitionKind::Method, "read", 0)
                .unwrap(),
            host_method: method.id,
        }],
    );
    implementation
        .associated_types
        .push(HostAssociatedTypeBinding {
            declaration: definitions
                .intern_child(trait_id, DefinitionKind::AssociatedType, "Item", 0)
                .unwrap(),
            ty: HostValueType::I32,
        });
    host.trait_implementations.push(implementation);
    host.methods.push(method);
    let make = HostFunctionDeclaration::new(
        &definitions,
        "demo.make",
        vec![],
        HostValueType::Opaque(host.id),
    )
    .unwrap();
    let interface = HostInterface {
        types: vec![host.clone()],
        functions: vec![make.clone()],
        paths: vec![],
    };
    // Reading these declarations does not register any runtime callback.
    let interface =
        HostInterface::from_bytes(&interface.to_bytes(&definitions.snapshot()).unwrap()).unwrap();
    engine.set_host_interface(interface).unwrap();
    let checked = engine
        .compile_snapshot(engine.source_snapshot(), file, &Default::default())
        .unwrap();
    let artifact = engine.emit_bytecode(&checked, Default::default()).unwrap();
    let host =
        DefinitionMetadata::checked(definitions.snapshot(), host, &Default::default()).unwrap();
    let make =
        DefinitionMetadata::checked(definitions.snapshot(), make, &Default::default()).unwrap();
    (engine, artifact, host, make)
}

fn context(jit: bool) -> ExecutionContext {
    ExecutionContext {
        jit_policy: if jit {
            JitPolicy::Enabled
        } else {
            JitPolicy::Disabled
        },
        ..Default::default()
    }
}

fn main() {
    // Compilation consumes only the offline declarations, before any callbacks exist.
    let (engine, artifact, host, make) = compile_offline();
    let context = context(false);
    let mut runtime = engine.runtime(context.clone());
    let ty = runtime
        .register_host_type(HostTypeRegistration::new(host.clone(), "Counter"))
        .unwrap();
    let root = runtime
        .runtime_mut()
        .register_host_root(HostObjectId(7), ty, HostSchemaEpoch::new(0))
        .unwrap();
    runtime
        .register_host_function(HostFunction::new(make, move |cx, _| {
            cx.runtime()
                .gc()
                .alloc_host_root(root)
                .map_err(|error| HostError::new(error.to_string()))
        }))
        .unwrap();
    runtime
        .register_host_function(
            HostFunction::method(&host, host.records().methods[0].id, |_, args| Ok(args[1]))
                .unwrap(),
        )
        .unwrap();
    let artifact =
        BytecodeArtifact::from_bytes(&artifact.records().to_bytes(artifact.definitions()).unwrap())
            .unwrap();
    let program =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    let result = runtime
        .execute(&loaded, ProgramEntry::Main, &[], &context)
        .unwrap();
    assert_eq!(
        result
            .return_value
            .value(runtime.runtime().gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
    println!(
        "static + dynamic host interface result: {:?}",
        result
            .return_value
            .value(runtime.runtime().gc())
            .expect("retained execution result")
    );
}

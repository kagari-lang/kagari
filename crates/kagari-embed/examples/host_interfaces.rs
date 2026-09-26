//! Offline host associated-output declarations and dynamic interface binding.
//! Run with `cargo run -p kagari-embed --example host_interfaces`.
use kagari_common::{
    host_interface::{
        HostAssociatedTypeBinding, HostFunctionDeclaration, HostInterface, HostMethodDeclaration,
        HostParameter, HostPassingStyle, HostTraitImplementationDeclaration,
        HostTraitMethodBinding, HostTypeDeclaration, HostTypeOwnership, HostValueType,
    },
    identity::{DefinitionId, DefinitionKind, DefinitionPathSegment},
    source_database::SourceLayer,
};
use kagari_embed::{
    BytecodeArtifact, CompileOptions, ExecutionContext, HostExposurePolicy, KagariEngine,
};
use kagari_runtime::{
    CapabilitySet, LanguageProfile,
    host::{HostFunction, HostObjectId, HostSchemaEpoch, HostTypeRegistration},
    value::Value,
};

const SOURCE: &str = include_str!("../../../examples/host-interfaces.kgr");

fn member(owner: &DefinitionId, kind: DefinitionKind, name: &str) -> DefinitionId {
    let mut id = owner.clone();
    id.path.push(DefinitionPathSegment {
        kind,
        name: name.into(),
        occurrence: 0,
    });
    id
}

fn compile_offline() -> (
    KagariEngine,
    BytecodeArtifact,
    HostTypeDeclaration,
    HostFunctionDeclaration,
) {
    let engine = KagariEngine::default();
    let file = engine
        .set_source("mem://host-interface", SOURCE.into(), SourceLayer::Base)
        .unwrap();
    let trait_id = DefinitionId {
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
    let mut host = HostTypeDeclaration::new("demo.Counter");
    host.ownership = HostTypeOwnership::HostRoot;
    host.path_access = kagari_common::host_interface::PathAccess::ReadOnly;
    let method = HostMethodDeclaration::new(
        &host.id,
        "read",
        vec![HostParameter {
            name: "amount".into(),
            ty: HostValueType::I32,
            passing: HostPassingStyle::Owned,
        }],
        HostValueType::I32,
    );
    let mut implementation = HostTraitImplementationDeclaration::new(
        trait_id.clone(),
        vec![],
        vec![HostTraitMethodBinding {
            trait_method: member(&trait_id, DefinitionKind::Method, "read"),
            host_method: method.id.clone(),
        }],
    );
    implementation
        .associated_types
        .push(HostAssociatedTypeBinding {
            declaration: member(&trait_id, DefinitionKind::AssociatedType, "Item"),
            ty: HostValueType::I32,
        });
    host.trait_implementations.push(implementation);
    host.methods.push(method);
    let make =
        HostFunctionDeclaration::new("demo.make", vec![], HostValueType::Opaque(host.id.clone()));
    let interface = HostInterface {
        types: vec![host.clone()],
        functions: vec![make.clone()],
        paths: vec![],
    };
    // Reading these declarations does not register any runtime callback.
    let interface = HostInterface::from_bytes(&interface.to_bytes().unwrap()).unwrap();
    engine.set_host_interface(interface).unwrap();
    let checked = engine
        .compile_snapshot(
            engine.source_snapshot(),
            file,
            CompileOptions {
                language_profile: LanguageProfile {
                    allow_host_calls: true,
                    allow_jit: true,
                    ..Default::default()
                },
            },
            &Default::default(),
        )
        .unwrap();
    let artifact = engine.emit_bytecode(&checked, Default::default()).unwrap();
    (engine, artifact, host, make)
}

fn context(jit: bool) -> ExecutionContext {
    ExecutionContext {
        language_profile: LanguageProfile {
            allow_host_calls: true,
            allow_jit: jit,
            ..Default::default()
        },
        capabilities: CapabilitySet {
            host_calls: true,
            jit,
            ..Default::default()
        },
        host_policy: HostExposurePolicy {
            allowed_host_functions: vec!["demo.make".into(), "demo.Counter.read".into()],
            ..Default::default()
        },
        jit_policy: if jit {
            kagari_embed::JitPolicy::Enabled
        } else {
            kagari_embed::JitPolicy::Disabled
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
        .register_host_function(HostFunction::new(make, move |_, _| {
            Ok(Value::HostRoot(root))
        }))
        .unwrap();
    runtime
        .register_host_function(
            HostFunction::method(&host, &host.methods[0].id, |_, args| Ok(args[1].clone()))
                .unwrap(),
        )
        .unwrap();
    let artifact = BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    let loaded = runtime.load_program(artifact, Default::default()).unwrap();
    let result = runtime.execute(&loaded, "main", &[], &context).unwrap();
    assert_eq!(result.return_value, Value::I32(42));
    println!(
        "static + dynamic host interface result: {:?}",
        result.return_value
    );
}

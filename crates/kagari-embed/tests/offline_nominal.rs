use kagari_common::{
    SourceFile,
    host_interface::{
        HostFieldDeclaration, HostFunctionDeclaration, HostInterface, HostParameter,
        HostPassingStyle, HostTypeDeclaration, HostTypeOwnership, HostValueType,
    },
};
use kagari_embed::{
    ArtifactOptions, CompileOptions, ExecutionContext, HostExposurePolicy, KagariEngine,
};
use kagari_runtime::{
    CapabilitySet, LanguageProfile,
    host::{HostFunction, HostObjectId, HostSchemaEpoch, HostTypeRegistration},
    value::Value,
};
use std::{cell::RefCell, rc::Rc};

fn interface() -> HostInterface {
    let related = HostTypeDeclaration::new("right.Item");
    let mut item = HostTypeDeclaration::new("left.Item");
    item.ownership = HostTypeOwnership::HostRoot;
    item.path_access = kagari_common::host_interface::PathAccess::ReadOnly;
    item.fields.push(HostFieldDeclaration::new(
        &item.id,
        "related",
        HostValueType::Opaque(related.id.clone()),
    ));
    let make =
        HostFunctionDeclaration::new("left.make", vec![], HostValueType::Opaque(item.id.clone()));
    let take = HostFunctionDeclaration::new(
        "left.take",
        vec![HostParameter {
            name: "value".into(),
            ty: HostValueType::Opaque(item.id.clone()),
            passing: HostPassingStyle::SharedBorrow,
        }],
        HostValueType::I32,
    );
    HostInterface {
        types: vec![item, related, HostTypeDeclaration::new("unused.Other")],
        functions: vec![make, take],
    }
}

#[test]
fn source_host_handles_link_offline_contracts_and_execute_across_backends() {
    let interface = interface();
    let engine = KagariEngine::default();
    engine
        .set_host_interface(HostInterface::from_bytes(&interface.to_bytes().unwrap()).unwrap())
        .unwrap();
    let profile = LanguageProfile {
        allow_host_calls: true,
        allow_jit: true,
        ..Default::default()
    };
    let artifact = engine.compile_to_artifact(
        SourceFile::new("nominal.kgr", "use left::Item; use left as api; pub fn pass(value: Item) -> api::Item { value } fn id<T>(value: T) -> T { value } fn main() -> i32 { val value: Item = api::make(); api::take(pass(id(value))) }"),
        CompileOptions { language_profile: profile }, ArtifactOptions::default(),
    ).unwrap();
    let module = &artifact.program.modules[artifact.program.root.index()];
    assert_eq!(module.host_interface.types, interface.types[..2]);
    let kagari_ir::module::PublicAbiItem::Function(pass) = &module.public_items[0] else {
        panic!("public pass")
    };
    assert_eq!(
        pass.return_type,
        kagari_ir::module::abi::AbiType::Host(interface.types[0].id.clone())
    );
    assert_eq!(
        pass.return_type.representation(),
        kagari_ir::module::ValueType::HostHandle
    );
    let mut invalid = artifact.program.clone();
    invalid.modules[artifact.program.root.index()]
        .host_interface
        .types
        .clear();
    assert!(kagari_ir::bytecode::KbcArtifact::from_program(invalid, Default::default()).is_err());
    for (encoded, jit) in [(false, false), (true, false), (true, true)] {
        let artifact = if encoded {
            kagari_ir::bytecode::KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap()
        } else {
            artifact.clone()
        };
        let context = ExecutionContext {
            language_profile: profile,
            capabilities: CapabilitySet {
                host_calls: true,
                jit: true,
                ..Default::default()
            },
            host_policy: HostExposurePolicy {
                allowed_host_functions: vec!["left.make".into(), "left.take".into()],
                ..Default::default()
            },
            jit_policy: if jit {
                kagari_embed::JitPolicy::Enabled
            } else {
                kagari_embed::JitPolicy::Disabled
            },
            ..Default::default()
        };
        let mut runtime = engine.runtime(context.clone());
        let ids = runtime
            .register_host_types(
                interface.types[..2]
                    .iter()
                    .cloned()
                    .map(|ty| HostTypeRegistration::new(ty, "Object"))
                    .collect(),
            )
            .unwrap();
        let root = runtime
            .runtime_mut()
            .register_host_root(HostObjectId(7), ids[0], HostSchemaEpoch::new(0))
            .unwrap();
        let trace = Rc::new(RefCell::new(Vec::new()));
        let calls = trace.clone();
        runtime
            .register_host_function(HostFunction::new(
                interface.functions[0].clone(),
                move |_, _| {
                    calls.borrow_mut().push("make");
                    Ok(Value::HostRoot(root))
                },
            ))
            .unwrap();
        let calls = trace.clone();
        runtime
            .register_host_function(HostFunction::new(
                interface.functions[1].clone(),
                move |context, args| {
                    calls.borrow_mut().push("take");
                    assert!(matches!(args[0], Value::HostRoot(_)));
                    context.runtime().collect_garbage().unwrap();
                    Ok(Value::I32(42))
                },
            ))
            .unwrap();
        let loaded = runtime.load_program(artifact, Default::default()).unwrap();
        assert!(trace.borrow().is_empty());
        let report = if jit {
            let mut backend = kagari_jit_cranelift::CraneliftBackend::for_host().unwrap();
            runtime.execute_with_backend(&loaded, "main", &[], &context, &mut backend)
        } else {
            runtime.execute(&loaded, "main", &[], &context)
        }
        .unwrap();
        assert_eq!(report.return_value, Value::I32(42));
        assert_eq!(*trace.borrow(), ["make", "take"]);
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
    }
}

#[test]
fn annotation_only_host_dependencies_are_verified_and_linked() {
    let engine = KagariEngine::default();
    let interface = interface();
    engine.set_host_interface(interface.clone()).unwrap();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "annotation.kgr",
                "pub fn pass(value: left::Item) -> left::Item { value } fn main() -> i32 { 7 }",
            ),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    let module = &artifact.program.modules[artifact.program.root.index()];
    assert!(module.host_interface.functions.is_empty());
    assert_eq!(module.host_interface.types.len(), 2);
    let mut runtime = engine.runtime(Default::default());
    assert!(
        runtime
            .load_program(artifact.clone(), Default::default())
            .is_err()
    );
    runtime
        .register_host_types(
            interface.types[..2]
                .iter()
                .cloned()
                .map(|ty| HostTypeRegistration::new(ty, "Object"))
                .collect(),
        )
        .unwrap();
    let loaded = runtime
        .load_program(artifact.clone(), Default::default())
        .unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, "main", &[], &Default::default())
            .unwrap()
            .return_value,
        Value::I32(7)
    );
    let mut invalid = artifact.program;
    invalid.modules[invalid.root.index()]
        .host_interface
        .types
        .clear();
    assert!(kagari_ir::bytecode::verify_program(&invalid).is_err());
}

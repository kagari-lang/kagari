//! Cold shared contracts, source-free loading and callback ownership boundaries.
mod support;

use kagari_bytecode::{
    artifact::{ArtifactBuildOptions, ArtifactCompatibility, KbcArtifact},
    instruction::ModuleSlot,
    program::BytecodeProgram,
};
use kagari_common::identity::{
    DefinitionKind, ModuleIdentity, PackageId, map::DefinitionContext,
    metadata::DefinitionMetadata, scoped::DefinitionReferences, table::DefinitionTable,
};
use kagari_runtime::{
    Runtime,
    error::RuntimeError,
    host::{HostCallContext, HostError},
    native::{builder::ModuleBuilder, module::NativeModule},
    value::Value,
};
use kagari_types::{
    callable::{CallableImplementation, MethodPolicy},
    collection::CollectionAccess,
    declaration::{
        FnDecl, Param, TypeDef, TypeDefKind,
        host::{
            HostCallPolicy, HostPassingStyle, HostReflectionPolicy, HostTypeOwnership,
            HostTypePolicy, PathAccess,
        },
        module::ModuleDecl,
    },
    language::binding,
    scalar::BuiltinType,
    ty::{NominalTy, Ty},
};
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
use support::native::{invoke, program::free_function_program};

pub fn scoped<T>(definitions: &DefinitionTable, records: T) -> DefinitionMetadata<T>
where
    T: DefinitionReferences,
{
    DefinitionMetadata::checked(definitions.clone(), records, &Default::default()).unwrap()
}

fn declaration(context: &DefinitionContext, package: &str) -> DefinitionMetadata<ModuleDecl> {
    let identity = ModuleIdentity {
        package: PackageId(package.into()),
        path: vec![],
    };
    let root = context.intern_root(&identity).unwrap();
    let echo = context
        .intern_child(root, DefinitionKind::Function, "echo", 0)
        .unwrap();
    let mut module = ModuleDecl::new(identity);
    module.functions.push(FnDecl {
        host_policy: Some(HostCallPolicy {
            passing: vec![HostPassingStyle::Owned],
            effects: Default::default(),
        }),
        method_policy: MethodPolicy::default(),
        name: "echo".into(),
        implementation: CallableImplementation::Native(echo),
        generic_params: vec![],
        bounds: vec![],
        params: vec![Param {
            name: "value".into(),
            ty: Ty::Builtin(BuiltinType::I32),
            mutable: false,
        }],
        return_type: Ty::Builtin(BuiltinType::I32),
    });
    module.collect_definitions(context).unwrap();
    scoped(&context.snapshot(), module)
}

fn bound(
    declarations: DefinitionMetadata<ModuleDecl>,
    callback: impl for<'call> Fn(&HostCallContext<'call>, &[Value]) -> Result<Value, HostError>
    + Send
    + Sync
    + 'static,
) -> Result<NativeModule, RuntimeError> {
    let mut builder =
        ModuleBuilder::from_declaration(declarations, &Default::default(), BTreeMap::new())?;
    let function = builder.declared_function("echo")?;
    builder.bind(function, callback)?;
    builder.finish()
}

fn artifact(program: DefinitionMetadata<BytecodeProgram>) -> DefinitionMetadata<KbcArtifact> {
    let context = DefinitionContext::new().unwrap();
    let program = program.import_into(&context, &Default::default()).unwrap();
    let artifact = KbcArtifact::from_program(
        program.into_records(),
        ArtifactBuildOptions::default(),
        &context,
    )
    .unwrap();
    scoped(&context.snapshot(), artifact)
}

fn encoded(declarations: &DefinitionMetadata<ModuleDecl>) -> Vec<u8> {
    let artifact = artifact(free_function_program(declarations));
    artifact.records().to_bytes(artifact.definitions()).unwrap()
}

#[test]
fn module_load_and_reload_require_matching_bindings_before_publication() {
    let context = DefinitionContext::new().unwrap();
    let required = declaration(&context, "game");
    let bytecode = free_function_program(&required);
    let mut runtime = Runtime::default();
    assert!(runtime.load_program("host", bytecode.clone()).is_err());
    assert_eq!(runtime.modules().loaded_count(), 0);
    bound(required.clone(), |_, _| {
        panic!("linking must not invoke Rust")
    })
    .unwrap()
    .install(&mut runtime)
    .unwrap();
    let loaded = runtime.load_program("host", bytecode).unwrap();
    let shared = runtime.modules().latest("host").unwrap();
    assert!(std::ptr::eq(&loaded.bytecode, &shared.bytecode));
    assert!(std::ptr::eq(&loaded.bytecode, &loaded.clone().bytecode));
    let before = runtime.resources().counters();
    let (definitions, mut mismatch) = required.into_parts();
    mismatch.functions[0]
        .host_policy
        .as_mut()
        .unwrap()
        .effects
        .may_mutate_host_state = true;
    let mismatch = free_function_program(&scoped(&definitions, mismatch));
    assert!(
        runtime
            .stage_reload_program(&loaded, "host", mismatch.clone())
            .is_err()
    );
    assert!(
        runtime
            .stage_reload_artifact(
                &loaded,
                "host",
                artifact(mismatch),
                &ArtifactCompatibility::default()
            )
            .is_err()
    );
    assert_eq!(
        runtime.modules().latest("host").unwrap().key(),
        loaded.key()
    );
    assert_eq!(runtime.resources().counters(), before);
}

#[test]
fn bound_slots_and_loaded_handles_reject_another_runtime() {
    let context = DefinitionContext::new().unwrap();
    let declaration = declaration(&context, "game");
    let module = bound(declaration.clone(), |_, args| Ok(args[0])).unwrap();
    let mut first = Runtime::default();
    let mut second = Runtime::default();
    module.install(&mut first).unwrap();
    module.install(&mut second).unwrap();
    let a = first
        .load_program("same", free_function_program(&declaration))
        .unwrap();
    let b = second
        .load_program("same", free_function_program(&declaration))
        .unwrap();
    // Native import slot zero exists in both programs, but its owner is mandatory.
    assert_eq!(a.bytecode.native_imports.len(), 1);
    assert_eq!(b.bytecode.native_imports.len(), 1);
    assert_eq!(a.key(), b.key());
    assert!(invoke(&second, &a, 0, &[Value::I32(1)]).is_err());
    assert!(second.validate_loaded_module(&a).is_err());
    assert!(second.module_instance_snapshot(&a).is_none());
    assert!(
        second
            .write_module_slot(&a, ModuleSlot::new(0), Value::Unit)
            .is_err()
    );
    assert!(second.validate_loaded_module(&b).is_ok());
    assert_eq!(
        invoke(&second, &b, 0, &[Value::I32(7)]).unwrap(),
        Value::I32(7)
    );
}

#[test]
fn callback_arguments_and_result_obey_the_declared_representation() {
    let context = DefinitionContext::new().unwrap();
    let declaration = declaration(&context, "game");
    let calls = Arc::new(AtomicUsize::new(0));
    let called = calls.clone();
    let module = bound(declaration.clone(), move |_, _| {
        called.fetch_add(1, Ordering::SeqCst);
        Ok(Value::Bool(true))
    })
    .unwrap();
    let mut runtime = Runtime::default();
    module.install(&mut runtime).unwrap();
    let loaded = runtime
        .load_program("host", free_function_program(&declaration))
        .unwrap();
    assert!(invoke(&runtime, &loaded, 0, &[]).is_err());
    assert!(invoke(&runtime, &loaded, 0, &[Value::Bool(true)]).is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(invoke(&runtime, &loaded, 0, &[Value::I32(1)]).is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(runtime.gc().active_roots(), 0);
}

#[test]
fn offline_roundtrip_and_binding_link_do_not_run_callbacks() {
    let context = DefinitionContext::new().unwrap();
    let prior = context.snapshot();
    let declaration = declaration(&context, "game");
    let program = free_function_program(&declaration);
    let encoded = encoded(&declaration);
    let decoded = KbcArtifact::from_bytes(&encoded).unwrap();
    let target = DefinitionContext::new().unwrap();
    let source = program.import_into(&target, &Default::default()).unwrap();
    let imported = decoded.import_into(&target, &Default::default()).unwrap();
    assert_eq!(
        imported.records().program.modules[0].native_declarations,
        source.records().modules[0].native_declarations
    );
    assert_eq!(
        imported.records().program.modules[0].native_imports,
        source.records().modules[0].native_imports
    );
    assert!(
        DefinitionMetadata::checked(prior, program.records().clone(), &Default::default()).is_err()
    );
    assert!(
        DefinitionMetadata::checked(
            target.snapshot(),
            program.records().clone(),
            &Default::default()
        )
        .is_err()
    );
    let calls = Arc::new(AtomicUsize::new(0));
    let called = calls.clone();
    let mut runtime = Runtime::default();
    bound(declaration, move |_, _| {
        called.fetch_add(1, Ordering::SeqCst);
        Ok(Value::I32(7))
    })
    .unwrap()
    .install(&mut runtime)
    .unwrap();
    let (definitions, decoded) = imported.into_parts();
    let loaded = runtime
        .load_program("offline", scoped(&definitions, decoded.program))
        .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_ne!(
        loaded.bytecode.native_imports[0].instance.declaration,
        program.records().modules[0].native_imports[0]
            .instance
            .declaration
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn linking_checks_identity_signature_borrow_and_effects() {
    let context = DefinitionContext::new().unwrap();
    let original = declaration(&context, "game");
    let mut runtime = Runtime::default();
    bound(original.clone(), |_, _| {
        panic!("linking must not invoke Rust")
    })
    .unwrap()
    .install(&mut runtime)
    .unwrap();
    let mut changes = vec![declaration(&context, "another-provider")];
    for change in [0, 1, 2] {
        let mut records = original.records().clone();
        let function = &mut records.functions[0];
        match change {
            0 => function.params[0].ty = Ty::Builtin(BuiltinType::Bool),
            1 => function.return_type = Ty::Builtin(BuiltinType::String),
            2 => {
                function
                    .host_policy
                    .as_mut()
                    .unwrap()
                    .effects
                    .may_mutate_host_state = true
            }
            _ => unreachable!(),
        }
        changes.push(scoped(original.definitions(), records));
    }
    for expected in changes {
        assert_ne!(encoded(&expected), encoded(&original));
        assert!(
            runtime
                .load_program("mismatch", free_function_program(&expected))
                .is_err()
        );
    }
    let mut text = declaration(&context, "text").into_records();
    text.functions[0].params[0].ty = Ty::Builtin(BuiltinType::String);
    let text = scoped(&context.snapshot(), text);
    bound(text.clone(), |_, _| panic!("linking must not invoke Rust"))
        .unwrap()
        .install(&mut runtime)
        .unwrap();
    let (definitions, mut borrowed) = text.into_parts();
    borrowed.functions[0].host_policy.as_mut().unwrap().passing[0] = HostPassingStyle::SharedBorrow;
    assert!(
        runtime
            .load_program(
                "borrow-mismatch",
                free_function_program(&scoped(&definitions, borrowed))
            )
            .is_err()
    );
    let mut docs = original.records().clone();
    let function = original
        .records()
        .native_declarations(original.definitions())
        .unwrap()[0]
        .declaration;
    docs.documentation
        .insert(function, "Updated help text".into());
    let docs = scoped(original.definitions(), docs);
    assert_eq!(encoded(&docs), encoded(&original));
    runtime
        .load_program("docs", free_function_program(&docs))
        .unwrap();
}

#[test]
fn offline_encoding_is_canonical_and_rejects_invalid_input() {
    let context = DefinitionContext::new().unwrap();
    let original = declaration(&context, "game");
    let other = DefinitionContext::new().unwrap();
    declaration(&other, "unrelated");
    let relocated = declaration(&other, "game");
    let first = encoded(&original);
    assert_eq!(first, encoded(&relocated));
    let mut trailing = first.clone();
    trailing.push(0);
    assert!(KbcArtifact::from_bytes(&trailing).is_err());
    assert!(KbcArtifact::from_bytes(&first[..8]).is_err());
    let mut bad_magic = first.clone();
    bad_magic[0] ^= 1;
    assert!(KbcArtifact::from_bytes(&bad_magic).is_err());
    let mut old_version = first;
    old_version[4..6].copy_from_slice(&0_u16.to_le_bytes());
    assert!(KbcArtifact::from_bytes(&old_version).is_err());
    let mut duplicate = original.records().clone();
    duplicate.functions.push(duplicate.functions[0].clone());
    let duplicate = free_function_program(&scoped(original.definitions(), duplicate));
    let context = DefinitionContext::new().unwrap();
    let duplicate = duplicate
        .import_into(&context, &Default::default())
        .unwrap();
    assert!(
        KbcArtifact::from_program(duplicate.into_records(), Default::default(), &context).is_err()
    );
}

#[test]
fn invalid_registration_leaves_registry_unchanged() {
    let context = DefinitionContext::new().unwrap();
    let original = declaration(&context, "game");
    let mut bad = original.records().clone();
    bad.functions[0].host_policy.as_mut().unwrap().passing[0] = HostPassingStyle::UniqueBorrow;
    assert!(bound(scoped(original.definitions(), bad), |_, _| unreachable!()).is_err());
    let mut runtime = Runtime::default();
    assert!(
        runtime
            .load_program("missing", free_function_program(&original))
            .is_err()
    );
    assert_eq!(runtime.modules().loaded_count(), 0);
    bound(original.clone(), |_, _| Ok(Value::I32(7)))
        .unwrap()
        .install(&mut runtime)
        .unwrap();
    let duplicate = bound(original.clone(), |_, _| Ok(Value::I32(9))).unwrap();
    assert!(duplicate.install(&mut runtime).is_err());
    let loaded = runtime
        .load_program("original", free_function_program(&original))
        .unwrap();
    assert_eq!(
        invoke(&runtime, &loaded, 0, &[Value::I32(1)]).unwrap(),
        Value::I32(7)
    );
}

#[test]
fn immutable_configuration_contracts_are_portable_and_reject_shared_objects() {
    let context = DefinitionContext::new().unwrap();
    let pure = declaration(&context, "game");
    let mut configuration = pure.records().clone();
    configuration.functions[0]
        .host_policy
        .as_mut()
        .unwrap()
        .effects
        .may_read_immutable_configuration = true;
    let configuration = scoped(pure.definitions(), configuration);
    assert_ne!(encoded(&pure), encoded(&configuration));
    let decoded = KbcArtifact::from_bytes(&encoded(&configuration)).unwrap();
    let target = DefinitionContext::new().unwrap();
    let decoded = decoded.import_into(&target, &Default::default()).unwrap();
    let source = free_function_program(&configuration)
        .import_into(&target, &Default::default())
        .unwrap();
    assert_eq!(
        decoded.records().program.modules[0].native_declarations,
        source.records().modules[0].native_declarations
    );
    let mut runtime = Runtime::default();
    bound(pure, |_, _| panic!("effect mismatch must not call Rust"))
        .unwrap()
        .install(&mut runtime)
        .unwrap();
    assert!(
        runtime
            .load_program(
                "configuration-mismatch",
                free_function_program(&configuration)
            )
            .is_err()
    );
    let option = context.intern(&binding::option_declaration()).unwrap();
    let root = context
        .snapshot()
        .lookup_root(&configuration.records().identity)
        .unwrap();
    let object = context
        .intern_child(root, DefinitionKind::Struct, "Object", 0)
        .unwrap();
    let option_of = |argument| {
        Ty::Enum(NominalTy {
            declaration: option,
            arguments: vec![argument],
            associated_types: Default::default(),
        })
    };
    let mut records = configuration.into_records();
    records.types.push(TypeDef {
        name: "Object".into(),
        kind: TypeDefKind::Host(HostTypePolicy {
            ownership: HostTypeOwnership::Opaque,
            reflection: HostReflectionPolicy::Hidden,
            path_access: PathAccess::None,
        }),
        generic_params: vec![],
        bounds: vec![],
        fields: vec![],
        variants: vec![],
    });
    records.collect_definitions(&context).unwrap();
    for result in [
        Ty::Array(Box::new(Ty::Builtin(BuiltinType::I32))),
        Ty::Tuple(vec![Ty::Set(
            Box::new(Ty::Builtin(BuiltinType::I32)),
            CollectionAccess::Mutable,
        )]),
        option_of(Ty::Host(object)),
    ] {
        let mut invalid = records.clone();
        invalid.functions[0].return_type = result;
        assert!(invalid.validate(&|_| None, &context.snapshot()).is_err());
    }
    let mut invalid = records.clone();
    invalid.functions[0].params[0].ty = Ty::Array(Box::new(Ty::Builtin(BuiltinType::I32)));
    assert!(invalid.validate(&|_| None, &context.snapshot()).is_err());
    records.functions[0].return_type = option_of(Ty::Tuple(vec![
        Ty::Builtin(BuiltinType::String),
        Ty::Builtin(BuiltinType::I32),
    ]));
    records.validate(&|_| None, &context.snapshot()).unwrap();
}

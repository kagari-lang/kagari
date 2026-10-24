#![cfg(feature = "source")]
#[path = "../../../../../tests/support/provider_contracts.rs"]
mod contracts;
mod external;
mod results;
// The source emitter and standalone artifact consumer share the reviewed provider.
mod library;
#[path = "../../../../../tests/support/native_provider.rs"]
mod provider;
use crate::{
    context::ExecutionContext,
    engine::{EngineConfig, KagariEngine},
    program::PreparedProgram,
    test_support::cancellation::CancelAt,
};
use contracts::alter_bindings;
use kagari_bytecode::artifact::KbcArtifact;
use kagari_common::identity::{
    map::DefinitionContext, metadata::DefinitionMetadata, table::DefinitionId,
};
use kagari_contract::types::PublicItem;
use kagari_runtime::{
    module::entry::ProgramEntry,
    native::{
        binding::NativeResult, builder::ModuleBuilder, context::CallContext,
        declarations::FunctionDecl, types::Type,
    },
    value::Value,
};
use kagari_source::source::SourceFile;
use kagari_types::{declaration::FnDecl, scalar::BuiltinType, ty::Ty};

fn engine(config: EngineConfig) -> KagariEngine {
    {
        let mut builder = KagariEngine::builder().unwrap();
        builder.config(config);
        builder
            .install(provider::module(Default::default()))
            .unwrap();
        builder.build().unwrap()
    }
}

fn artifact(source: &str) -> DefinitionMetadata<KbcArtifact> {
    engine(EngineConfig::default())
        .compile_to_artifact(
            SourceFile::new(
                "memory://provider-reset.kgr",
                format!("use external::fixture as native;\n{source}"),
            ),
            Default::default(),
        )
        .unwrap()
}

fn execute(source: &str) {
    let artifact = artifact(source);
    let bytes = artifact.records().to_bytes(artifact.definitions()).unwrap();
    let program = PreparedProgram::from_artifact(
        KbcArtifact::from_bytes(&bytes).unwrap(),
        &Default::default(),
        &Default::default(),
    )
    .unwrap();
    let context = ExecutionContext::default();
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let mut runtime = engine(config).runtime(context.clone());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    for _ in 0..3 {
        assert_eq!(
            runtime
                .execute(&loaded, ProgramEntry::Main, &[], &context)
                .unwrap()
                .return_value
                .value(runtime.runtime().gc())
                .expect("retained execution result"),
            Value::I32(42)
        );
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
        assert_eq!(
            runtime.runtime().resources().counters().current_call_depth,
            0
        );
        runtime.runtime().collect_garbage().unwrap();
        assert_eq!(runtime.runtime().gc().allocated_objects(), 0);
    }
}

#[test]
fn minimal_array_provider_executes_from_serialized_artifact() {
    execute(
        "fn main() -> i32 { val a: Vec<i32> = Vec::new(); a.push(20); a.push(22); if a.len() == 2u64 { a[0u64] + a[1u64] } else { 0 } }",
    );
}

#[test]
fn provider_callback_retains_captures_partial_output_and_heap_results() {
    execute(
        r#"fn main() -> i32 {
        val count = [18];
        val arrays = native::from_fn(2u64, |index| {
            count[0u64] = count[0u64] + 2;
            [count[0u64]]
        });
        arrays[0u64][0u64] + arrays[1u64][0u64]
    }"#,
    );
}

#[test]
fn zero_length_initialization_does_not_call_the_callback() {
    execute(
        r#"fn main() -> i32 {
        val count = [42];
        val a = native::from_fn(0u64, |index| { count[0u64] = 0; 1 });
        if a.len() == 0u64 { count[0u64] } else { 0 }
    }"#,
    );
}

#[test]
fn readonly_interfaces_expose_reads_and_hide_mutators_without_native_access_flags() {
    execute(
        "fn main() -> i32 { val a = [20, 22]; val view: [i32] = a; if view.len() == 2u64 { view[0u64] + view[1u64] } else { 0 } }",
    );
    assert!(
        KagariEngine::default()
            .compile_to_artifact(
                SourceFile::new(
                    "memory://readonly.kgr",
                    "fn main() { val view: std::collections::List<i32> = Vec::from([1]); view.push(2); }"
                ),
                Default::default(),
            )
            .is_err()
    );
}

#[test]
fn readonly_list_methods_observe_mutation_through_a_concrete_alias() {
    execute(
        r#"use std::collections::{List};
fn main() -> i32 {
            val storage = Vec::from([20]);
            val view: List<i32> = storage;
            storage.push(22);
            match view.get(1u64) {
                Some(value) => view[0u64] + value,
                None => 0,
            }
        }"#,
    );
}

#[test]
fn structural_agreement_does_not_authorize_an_unknown_or_wrong_native_entry() {
    let baseline = artifact("fn main() -> u64 { [1, 2].len() }");
    for change in 0..3 {
        let context = DefinitionContext::new().unwrap();
        let mut original = baseline
            .import_into(&context, &Default::default())
            .unwrap()
            .into_records();
        alter_bindings(&mut original, |id| {
            *id = altered_binding(&context, *id, change)
        });
        // Consistent unsigned contract assertions remain structurally checkable.
        let forged = KbcArtifact::from_program(
            original.program,
            Default::default(),
            &context,
            &Default::default(),
        )
        .unwrap();
        let program = PreparedProgram::from_artifact(
            DefinitionMetadata::checked(context.snapshot(), forged, &Default::default()).unwrap(),
            &Default::default(),
            &Default::default(),
        )
        .unwrap();
        let context = ExecutionContext::default();
        let mut runtime = engine(EngineConfig::default()).runtime(context);
        assert!(runtime.load_program(&program, Default::default()).is_err());
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
        assert_eq!(
            runtime.runtime().resources().counters().current_call_depth,
            0
        );
    }
}

#[test]
fn a_forged_source_signature_cannot_change_the_installed_native_signature() {
    fn change_result(function: &mut FnDecl) {
        if matches!(function.name.as_str(), "number" | "main")
            && function.return_type == Ty::Builtin(BuiltinType::U8)
        {
            function.return_type = Ty::Builtin(BuiltinType::U16);
        }
    }
    let mut builder = KagariEngine::builder().unwrap();
    let mut module = ModuleBuilder::new("fixture", builder.declarations()).unwrap();
    let function = module
        .define_function(FunctionDecl::new("number").returns(Type::scalar(BuiltinType::U8)))
        .unwrap();
    module
        .bind(function, |_: &mut CallContext<'_>| -> NativeResult<u8> {
            Ok(0)
        })
        .unwrap();
    builder.install(module.finish().unwrap()).unwrap();
    let engine = builder.build().unwrap();
    let context = DefinitionContext::new().unwrap();
    let mut original = engine
        .compile_to_artifact(
            SourceFile::new(
                "forged-signature.kgr",
                "fn main() -> u8 { fixture::number() }",
            ),
            Default::default(),
        )
        .unwrap()
        .import_into(&context, &Default::default())
        .unwrap()
        .into_records();
    let definitions = context.snapshot();
    for module in &mut original.program.modules {
        for import in &mut module.native_imports {
            if definitions
                .resolve(import.instance.declaration)
                .unwrap()
                .last()
                .is_some_and(|part| part.name == "number")
                && import.signature.result == Ty::Builtin(BuiltinType::U8)
            {
                import.signature.result = Ty::Builtin(BuiltinType::U16);
            }
        }
        for declaration in &mut module.native_declarations {
            change_result(&mut declaration.function);
        }
        for contract in &mut module.trait_contracts {
            for method in &mut contract.abi.methods {
                change_result(method);
            }
        }
        for item in &mut module.public_items {
            match item {
                PublicItem::Function(function) => change_result(function),
                PublicItem::Trait(record) => {
                    for method in &mut record.methods {
                        change_result(method);
                    }
                }
                PublicItem::InherentTable(table) => {
                    for method in &mut table.methods {
                        change_result(method);
                    }
                }
                PublicItem::InterfaceTable(table) => {
                    for method in &mut table.methods {
                        change_result(method);
                    }
                }
                _ => {}
            }
        }
        for function in &mut module.functions {
            if !function.identity.as_ref().is_some_and(|identity| {
                definitions
                    .resolve(identity.declaration)
                    .unwrap()
                    .last()
                    .is_some_and(|part| matches!(part.name, "number" | "main"))
            }) {
                continue;
            }
            let semantic = &mut function.metadata.semantic;
            if semantic.result == Some(Ty::Builtin(BuiltinType::U8)) {
                semantic.result = Some(Ty::Builtin(BuiltinType::U16));
            }
            for ty in semantic.slots.values_mut() {
                if *ty == Ty::Builtin(BuiltinType::U8) {
                    *ty = Ty::Builtin(BuiltinType::U16);
                }
            }
        }
    }
    // Alter only the native result and its caller, leaving the binding identity intact.
    // u8 and u16 share a representation: portable checks accept the forgery,
    // but installation must check the source ABI.
    let forged = KbcArtifact::from_program(
        original.program,
        Default::default(),
        &context,
        &Default::default(),
    )
    .unwrap();
    let program = PreparedProgram::from_artifact(
        DefinitionMetadata::checked(context.snapshot(), forged, &Default::default()).unwrap(),
        &Default::default(),
        &Default::default(),
    )
    .unwrap();
    let mut runtime = engine.runtime(Default::default());
    assert!(runtime.load_program(&program, Default::default()).is_err());
    assert_eq!(runtime.runtime().gc().active_roots(), 0);
}

#[test]
fn callbacks_abort_without_retaining_native_state_or_partial_arrays() {
    let original = artifact(
        "fn main() -> i32 { val values = native::from_fn(3u64, |i| { if i == 1u64 { 1 / 0 } else { 20 } }); values[0u64] }",
    );
    let program =
        PreparedProgram::from_artifact(original, &Default::default(), &Default::default()).unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine(EngineConfig::default()).runtime(context.clone());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    assert!(
        runtime
            .execute(&loaded, ProgramEntry::Main, &[], &context)
            .is_err()
    );
    assert_eq!(runtime.runtime().gc().active_roots(), 0);
    assert_eq!(
        runtime.runtime().resources().counters().current_call_depth,
        0
    );
    runtime.runtime().collect_garbage().unwrap();
    assert_eq!(runtime.runtime().gc().allocated_objects(), 0);
    assert!(!runtime.runtime().is_quarantined());
}

#[test]
fn cancellation_boundaries_unwind_nested_provider_callbacks() {
    let program = PreparedProgram::from_artifact(
        artifact("fn main() -> i32 { val values = native::from_fn(2u64, |i| { native::from_fn(1u64, |j| { [21] }) }); values[0u64][0u64][0u64] + values[1u64][0u64][0u64] }"),
        &Default::default(), &Default::default(),
    ).unwrap();
    let mut cancelled = 0;
    let mut finished = false;
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    // This scenario calls only the stateless from_fn callback. Each attempt still
    // gets fresh runtime state, heap, cancellation token and observer.
    let engine = engine(config);
    for at in 0..200 {
        let context = ExecutionContext::default();
        let mut runtime = engine.runtime(context.clone());
        let loaded = runtime.load_program(&program, Default::default()).unwrap();
        let _selected_loaded = runtime.runtime().select_program(&loaded).unwrap();
        let options = kagari_runtime::session::ExecutionOptions {
            cancellation: context.cancellation.clone(),
            ..Default::default()
        };
        runtime
            .runtime()
            .set_execution_observer(CancelAt {
                seen: Default::default(),
                at,
                token: context.cancellation.clone(),
            })
            .unwrap();
        let session = runtime
            .runtime()
            .begin_execution(&_selected_loaded, options)
            .unwrap();
        runtime.runtime().attach_execution_observer().unwrap();
        match runtime.execute(&loaded, ProgramEntry::Main, &[], &context) {
            Ok(report) => {
                assert_eq!(
                    report
                        .return_value
                        .value(runtime.runtime().gc())
                        .expect("retained execution result"),
                    Value::I32(42)
                );
                finished = true;
            }
            Err(error) => {
                assert_eq!(error.code(), "KG_RUNTIME_CANCELLED");
                cancelled += 1;
            }
        }
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
        assert_eq!(
            runtime.runtime().resources().counters().current_call_depth,
            0
        );
        drop(session);
        runtime.runtime().collect_garbage().unwrap();
        assert_eq!(runtime.runtime().gc().allocated_objects(), 0);
        assert!(!runtime.runtime().is_quarantined());
        if finished {
            break;
        }
    }
    assert!(finished);
    assert!(cancelled > 20);
}

/// Forge a binding at its authoring boundary without changing the declaration it claims to implement.
fn altered_binding(context: &DefinitionContext, id: DefinitionId, change: usize) -> DefinitionId {
    let definitions = context.snapshot();
    let view = definitions.resolve(id).unwrap();
    let mut owner = view.module().clone();
    if change == 0 {
        owner.package.0.push_str("-unknown");
    }
    let mut parent = context.intern_root(&owner).unwrap();
    let segments: Vec<_> = view.segments().collect();
    for (index, segment) in segments.iter().enumerate() {
        let name = if index + 1 == segments.len() && change != 0 {
            if change == 1 {
                format!("{}-unknown", segment.name)
            } else {
                "is_empty".to_owned()
            }
        } else {
            segment.name.to_owned()
        };
        parent = context
            .intern_child(parent, segment.kind, &name, segment.occurrence)
            .unwrap();
    }
    parent
}

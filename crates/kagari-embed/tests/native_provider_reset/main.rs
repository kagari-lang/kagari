#![cfg(feature = "source")]
mod contracts;
mod external;
// The source emitter and standalone artifact consumer share the reviewed provider.
mod library;
#[path = "../support/native_provider.rs"]
mod provider;
use contracts::alter_bindings;
use kagari_abi::{
    scalar::BuiltinType,
    types::{AbiType, FunctionAbi, PublicAbiItem},
};
use kagari_bytecode::artifact::KbcArtifact;
use kagari_common::source::SourceFile;
use kagari_embed::{
    context::ExecutionContext,
    engine::{EngineConfig, KagariEngine},
    program::PreparedProgram,
};
use kagari_runtime::value::Value;

fn engine(config: EngineConfig) -> KagariEngine {
    KagariEngine::builder()
        .config(config)
        .install(provider::module(Default::default()))
        .build()
        .unwrap()
}

fn artifact(source: &str) -> KbcArtifact {
    engine(EngineConfig::default())
        .compile_to_artifact(
            SourceFile::new(
                "memory://provider-reset.kgr",
                format!("use external::fixture as native;\n{source}"),
            ),
            Default::default(),
            Default::default(),
        )
        .unwrap()
}

fn execute(source: &str) {
    let bytes = artifact(source).to_bytes().unwrap();
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
                .execute(&loaded, "main", &[], &context)
                .unwrap()
                .return_value,
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
        "fn main() -> i32 { val a: ArrayList<i32> = ArrayList::new(); a.push(20); a.push(22); if a.len() == 2usize { a[0usize] + a[1usize] } else { 0 } }",
    );
}

#[test]
fn provider_callback_retains_captures_partial_output_and_heap_results() {
    execute(
        r#"fn main() -> i32 {
        val count = [18];
        val arrays = native::from_fn(2usize, |index| {
            count[0usize] = count[0usize] + 2;
            [count[0usize]]
        });
        arrays[0usize][0usize] + arrays[1usize][0usize]
    }"#,
    );
}

#[test]
fn zero_length_initialization_does_not_call_the_callback() {
    execute(
        r#"fn main() -> i32 {
        val count = [42];
        val a = native::from_fn(0usize, |index| { count[0usize] = 0; 1 });
        if a.len() == 0usize { count[0usize] } else { 0 }
    }"#,
    );
}

#[test]
fn readonly_interfaces_expose_reads_and_hide_mutators_without_native_access_flags() {
    execute(
        "fn main() -> i32 { val a = [20, 22]; val view: [i32] = a; if view.len() == 2usize { view[0usize] + view[1usize] } else { 0 } }",
    );
    assert!(
        KagariEngine::default()
            .compile_to_artifact(
                SourceFile::new(
                    "memory://readonly.kgr",
                    "fn main() { val view: [i32] = [1]; view.push(2); }"
                ),
                Default::default(),
                Default::default(),
            )
            .is_err()
    );
}

#[test]
fn readonly_list_methods_observe_mutation_through_a_concrete_alias() {
    execute(
        r#"fn main() -> i32 {
            val storage = [20];
            val view: List<i32> = storage;
            storage.push(22);
            match view.get(1usize) {
                Some(value) => view[0usize] + value,
                None => 0,
            }
        }"#,
    );
}

#[test]
fn structural_agreement_does_not_authorize_an_unknown_or_wrong_native_entry() {
    for change in 0..3 {
        let mut original = artifact("fn main() -> usize { [1, 2].len() }");
        alter_bindings(&mut original, |id| match change {
            0 => id.module.package.0.push_str("-unknown"),
            1 => id.path.last_mut().unwrap().name.push_str("-unknown"),
            _ => id.path.last_mut().unwrap().name = "$foundation_list_new".into(),
        });
        // Consistent unsigned contract assertions remain structurally checkable.
        let forged = KbcArtifact::from_program(original.program, Default::default()).unwrap();
        let program =
            PreparedProgram::from_artifact(forged, &Default::default(), &Default::default())
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
    fn change_result(function: &mut FunctionAbi) {
        if function.return_type == AbiType::Builtin(BuiltinType::USize) {
            function.return_type = AbiType::Builtin(BuiltinType::U64);
        }
    }
    let mut original = artifact("fn main() -> usize { [1, 2].len() }");
    for module in &mut original.program.modules {
        for import in &mut module.native_imports {
            if import.signature.result == AbiType::Builtin(BuiltinType::USize) {
                import.signature.result = AbiType::Builtin(BuiltinType::U64);
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
                PublicAbiItem::Function(function) => change_result(function),
                PublicAbiItem::Trait(record) => {
                    for method in &mut record.methods {
                        change_result(method);
                    }
                }
                PublicAbiItem::InterfaceTable(table) => {
                    for method in &mut table.methods {
                        change_result(method);
                    }
                }
                _ => {}
            }
        }
        for function in &mut module.functions {
            let semantic = &mut function.metadata.semantic;
            if semantic.result == Some(AbiType::Builtin(BuiltinType::USize)) {
                semantic.result = Some(AbiType::Builtin(BuiltinType::U64));
            }
            for ty in semantic.registers.values_mut() {
                if *ty == AbiType::Builtin(BuiltinType::USize) {
                    *ty = AbiType::Builtin(BuiltinType::U64);
                }
            }
        }
    }
    // usize and u64 have the same physical representation: portable checks can
    // accept this consistent forgery, but installation must check the source ABI.
    let forged = KbcArtifact::from_program(original.program, Default::default()).unwrap();
    let program =
        PreparedProgram::from_artifact(forged, &Default::default(), &Default::default()).unwrap();
    let mut runtime = engine(EngineConfig::default()).runtime(Default::default());
    assert!(runtime.load_program(&program, Default::default()).is_err());
    assert_eq!(runtime.runtime().gc().active_roots(), 0);
}

#[test]
fn callbacks_abort_without_retaining_native_state_or_partial_arrays() {
    let original = artifact(
        "fn main() -> i32 { val values = native::from_fn(3usize, |i| { if i == 1usize { 1 / 0 } else { 20 } }); values[0usize] }",
    );
    let program =
        PreparedProgram::from_artifact(original, &Default::default(), &Default::default()).unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine(EngineConfig::default()).runtime(context.clone());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    assert!(runtime.execute(&loaded, "main", &[], &context).is_err());
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
fn every_instruction_budget_boundary_unwinds_nested_provider_callbacks() {
    let program = PreparedProgram::from_artifact(
        artifact("fn main() -> i32 { val values = native::from_fn(2usize, |i| { native::from_fn(1usize, |j| { [21] }) }); values[0usize][0usize][0usize] + values[1usize][0usize][0usize] }"),
        &Default::default(), &Default::default(),
    ).unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine(EngineConfig::default()).runtime(context.clone());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, "main", &[], &context)
            .unwrap()
            .return_value,
        Value::I32(42)
    );
    let steps = runtime.runtime().resources().counters().instruction_steps;
    assert!(steps > 20);
    for limit in 0..=steps {
        let mut limited = ExecutionContext::default();
        limited.resources.max_instruction_steps = Some(limit);
        let mut config = EngineConfig::default();
        config.default_runtime.gc.collection_threshold = Some(1);
        let mut runtime = engine(config).runtime(limited.clone());
        let loaded = runtime.load_program(&program, Default::default()).unwrap();
        let result = runtime.execute(&loaded, "main", &[], &limited);
        if limit == steps {
            assert_eq!(result.unwrap().return_value, Value::I32(42));
        } else {
            assert!(result.is_err(), "budget {limit}");
        }
        assert_eq!(runtime.runtime().gc().active_roots(), 0, "budget {limit}");
        assert_eq!(
            runtime.runtime().resources().counters().current_call_depth,
            0
        );
        runtime.runtime().collect_garbage().unwrap();
        assert_eq!(
            runtime.runtime().gc().allocated_objects(),
            0,
            "budget {limit}"
        );
        assert!(!runtime.runtime().is_quarantined());
    }
}

//! Registered ordinary associated types use the shared static and offline contracts.
// Test/cross-target sharing keeps the fixture's application registration authoritative.
#[path = "fixtures/native_associated_api.rs"]
mod fixture_api;
use kagari_abi::{
    native_import::binding_id,
    scalar::BuiltinType,
    types::{AbiType, GenericParameterAbi},
};
use kagari_bytecode::{artifact::KbcArtifact, instruction::NativeImportId, module::CallableTarget};
use kagari_common::{
    identity::{DefinitionKind, DefinitionPathSegment, associated_type_id},
    span::Span,
};
use kagari_embed::{
    context::ExecutionContext,
    engine::{EngineConfig, KagariEngine},
    program::PreparedProgram,
};
use kagari_runtime::{Runtime, RuntimeConfig, native::packages::standard_library, value::Value};
use kagari_vm::vm::Vm;
use std::{cell::Cell, rc::Rc};

const ARTIFACT: &[u8] = include_bytes!("fixtures/native_associated.kbc");
fn engine(calls: Rc<Cell<usize>>) -> KagariEngine {
    configured_engine(calls, true)
}
fn configured_engine(calls: Rc<Cell<usize>>, defaults: bool) -> KagariEngine {
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    KagariEngine::builder()
        .config(config)
        .install_standard_library(defaults)
        .install(Ok(fixture_api::api(fixture_api::module(), calls)))
        .install(fixture_api::typed::native_api())
        .build()
        .unwrap()
}
fn prepared() -> PreparedProgram {
    PreparedProgram::from_artifact(
        KbcArtifact::from_bytes(ARTIFACT).unwrap(),
        &Default::default(),
        &Default::default(),
    )
    .unwrap()
}

#[test]
fn associated_declarations_and_bindings_render_checked_sites_and_derived_signatures() {
    let module = fixture_api::module();
    module.validate().unwrap();
    let source = module.declaration_source().unwrap();
    assert!(source.text.contains("type Item: Marker;"));
    assert!(
        source
            .text
            .contains("fn head(self) -> <Self as Source>::Item;")
    );
    assert!(source.text.contains(
        "impl<T0> Source for Bag<T0> {\n    type Item = T0;\n    pub fn head(self) -> T0;"
    ));
    assert!(source.text.contains("fn echo<T0>(value: <T0 as Source>::Item, source: T0) -> <T0 as Source>::Item where T0: Source;"));
    let item = associated_type_id(&module.definition(DefinitionKind::Trait, "Source"), "Item");
    let slot = &source.sites[&item];
    let text = |span: Span| &source.text[span.start..span.end];
    assert_eq!(text(slot.name_span), "Item");
    assert!(source.text[..slot.span.start].contains("/// The value produced by this source."));
    let marked = associated_type_id(&module.definition(DefinitionKind::Trait, "Marked"), "Item");
    assert_eq!(
        text(source.sites[&marked].bounds[0].constraints[0]),
        "Marker"
    );
    let applied = associated_type_id(&module.implementation_id(0), "Item");
    assert_eq!(text(source.sites[&applied].parameters[0]), "T0");
    assert_eq!(
        module.implementations[0].methods[0].return_type,
        module.implementations[0].generic_params[0].as_type()
    );
}

#[test]
fn invalid_associated_owners_outputs_families_and_method_contracts_are_rejected() {
    for case in 0..5 {
        let mut module = fixture_api::module();
        match case {
            0 => {
                module.traits[0].associated_types[0].declaration =
                    associated_type_id(&module.definition(DefinitionKind::Trait, "Other"), "Item")
            }
            1 => module.implementations[0]
                .trait_type
                .as_mut()
                .unwrap()
                .associated_types
                .clear(),
            2 => {
                module.implementations[0].methods[0].return_type =
                    AbiType::Builtin(BuiltinType::Bool)
            }
            3 => {
                let associated = &mut module.traits[0].associated_types[0];
                associated.generic_params.push(GenericParameterAbi {
                    owner: associated.declaration.clone(),
                    position: 0,
                });
            }
            4 => {
                let ty = module.implementations[0].trait_type.as_mut().unwrap();
                ty.associated_types.insert(
                    binding_id(&module.identity, "foreign"),
                    AbiType::Builtin(BuiltinType::I32),
                );
            }
            _ => unreachable!(),
        }
        assert!(module.validate().is_err(), "invalid associated case {case}");
    }
}

#[test]
fn encoded_associated_native_calls_normalize_outputs_and_release_generic_heap_roots() {
    let calls = Rc::new(Cell::new(0));
    let engine = engine(calls.clone());
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime
        .load_program(&prepared(), Default::default())
        .unwrap();
    assert_eq!(calls.get(), 0);
    assert_eq!(
        runtime
            .execute(&loaded, "main", &[], &context)
            .unwrap()
            .return_value,
        Value::I32(42)
    );
    assert_eq!(calls.get(), 3);
    assert_eq!(runtime.runtime().gc().active_roots(), 0);
    runtime.runtime().collect_garbage().unwrap();
    assert_eq!(runtime.runtime().gc().allocated_objects(), 0);
}

#[test]
fn typed_rust_associated_outputs_nested_values_and_self_parameters_execute_offline() {
    let api = fixture_api::typed::native_api().unwrap();
    let source = &api.declaration_sources()[0].text;
    assert!(source.contains("type Item;"));
    assert!(source.contains("fn head(self) -> <Self as TypedSource>::Item;"));
    assert!(source.contains("fn round_trip(self, value: Option<(<Self as TypedSource>::Item,)>) -> Option<(<Self as TypedSource>::Item,)>;"));
    assert!(source.contains("fn keep(self, other: Self) -> Self;"));
    assert!(source.contains("fn swap(self, item: <Self as Transform<T0>>::Item, other: T0) -> (T0, <Self as Transform<T0>>::Item);"));
    let engine = engine(Rc::new(Cell::new(0)));
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime
        .load_program(&prepared(), Default::default())
        .unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, "typed_main", &[], &context)
            .unwrap()
            .return_value,
        Value::I32(42)
    );
    assert_eq!(runtime.runtime().gc().active_roots(), 0);
    runtime.runtime().collect_garbage().unwrap();
    assert_eq!(runtime.runtime().gc().allocated_objects(), 0);
}

#[test]
fn typed_associated_failure_releases_native_frames_and_roots() {
    let engine = engine(Rc::new(Cell::new(0)));
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime
        .load_program(&prepared(), Default::default())
        .unwrap();
    assert!(
        runtime
            .execute(&loaded, "typed_empty", &[], &context)
            .is_err()
    );
    assert_eq!(runtime.runtime().gc().active_roots(), 0);
    assert_eq!(
        runtime.runtime().resources().counters().current_call_depth,
        0
    );
    runtime.runtime().collect_garbage().unwrap();
    assert_eq!(runtime.runtime().gc().allocated_objects(), 0);
}

#[test]
fn associated_native_packages_execute_offline_without_default_installation() {
    let calls = Rc::new(Cell::new(0));
    let engine = configured_engine(calls.clone(), false);
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime
        .load_program(&prepared(), Default::default())
        .unwrap();
    for function in ["main", "typed_main"] {
        assert_eq!(
            runtime
                .execute(&loaded, function, &[], &context)
                .unwrap()
                .return_value,
            Value::I32(42)
        );
    }
    assert_eq!(calls.get(), 3);
    assert_eq!(runtime.runtime().gc().active_roots(), 0);
}

#[test]
fn dynamic_native_slots_execute_and_release_frames_on_success_and_failure() {
    let calls = Rc::new(Cell::new(0));
    let engine = configured_engine(calls.clone(), false);
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime
        .load_program(&prepared(), Default::default())
        .unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, "dynamic_main", &[], &context)
            .unwrap()
            .return_value,
        Value::I32(42)
    );
    assert_eq!(calls.get(), 1);
    assert_eq!(runtime.runtime().gc().active_roots(), 0);
    let error = runtime
        .execute(&loaded, "dynamic_empty", &[], &context)
        .unwrap_err();
    let trace = error.error_trace().unwrap();
    assert_eq!(trace.frames[0].function_name, "head");
    assert!(matches!(trace.frames[0].target, CallableTarget::Native(_)));
    assert!(trace.frames[0].source_span.is_none());
    assert_eq!(trace.frames[1].function_name, "dynamic_empty");
    assert_eq!(calls.get(), 2);
    assert_eq!(runtime.runtime().gc().active_roots(), 0);
    assert_eq!(
        runtime.runtime().resources().counters().current_call_depth,
        0
    );
    runtime.runtime().collect_garbage().unwrap();
    assert_eq!(runtime.runtime().gc().allocated_objects(), 0);
}

#[test]
fn native_callbacks_select_script_or_native_interface_targets_with_heap_results() {
    let calls = Rc::new(Cell::new(0));
    let engine = configured_engine(calls.clone(), false);
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime
        .load_program(&prepared(), Default::default())
        .unwrap();
    for name in ["callback_native_main", "callback_script_main"] {
        assert_eq!(
            runtime
                .execute(&loaded, name, &[], &context)
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
    assert_eq!(calls.get(), 1);
    let error = runtime
        .execute(&loaded, "callback_native_empty", &[], &context)
        .unwrap_err();
    assert_eq!(error.error_trace().unwrap().frames[0].function_name, "head");
    assert_eq!(calls.get(), 2);
    assert_eq!(runtime.runtime().gc().active_roots(), 0);
    assert_eq!(
        runtime.runtime().resources().counters().current_call_depth,
        0
    );
}

#[test]
fn native_interface_frames_charge_continuations_and_clean_up_exhausted_budgets() {
    let calls = Rc::new(Cell::new(0));
    let engine = configured_engine(calls.clone(), false);
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime
        .load_program(&prepared(), Default::default())
        .unwrap();
    let mut entered_before_exhaustion = false;
    let mut finished = false;
    for limit in 0..40 {
        let mut limited = context.clone();
        limited.resources.max_instruction_steps = Some(limit);
        let before = calls.get();
        match runtime.execute(&loaded, "callback_native_main", &[], &limited) {
            Ok(report) => {
                assert_eq!(report.return_value, Value::I32(42));
                finished = true;
            }
            Err(error) => {
                assert_eq!(error.code(), "KG_RUNTIME_RESOURCE_LIMIT_EXCEEDED");
                entered_before_exhaustion |= calls.get() > before;
            }
        }
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
        assert_eq!(
            runtime.runtime().resources().counters().current_call_depth,
            0
        );
        assert!(!runtime.runtime().is_quarantined());
        runtime.runtime().collect_garbage().unwrap();
        assert_eq!(runtime.runtime().gc().allocated_objects(), 0);
        if finished {
            break;
        }
    }
    assert!(finished && entered_before_exhaustion);
    let before = calls.get();
    let mut shallow = context;
    shallow.resources.max_call_depth = Some(1);
    assert!(
        runtime
            .execute(&loaded, "dynamic_main", &[], &shallow)
            .is_err()
    );
    assert_eq!(calls.get(), before);
    assert_eq!(
        runtime.runtime().resources().counters().current_call_depth,
        0
    );
}

#[test]
fn external_native_interface_entry_retains_its_generation_across_reload() {
    let calls = Rc::new(Cell::new(0));
    let mut config = RuntimeConfig::default();
    config.gc.collection_threshold = Some(1);
    let mut runtime = Runtime::new(config);
    standard_library().install(&mut runtime).unwrap();
    fixture_api::api(fixture_api::module(), calls.clone())
        .install(&mut runtime)
        .unwrap();
    fixture_api::typed::native_api()
        .unwrap()
        .install(&mut runtime)
        .unwrap();
    let artifact = KbcArtifact::from_bytes(ARTIFACT).unwrap();
    let old = runtime
        .load_program("associated-reload", artifact.program.clone())
        .unwrap();
    let mut vm = Vm::new(runtime);
    let value = vm.execute(&old, "dynamic_boxed").unwrap().return_value;
    let root = vm.runtime().root_value(value.clone()).unwrap();
    let mut method = fixture_api::module().definition(DefinitionKind::Trait, "Source");
    method.path.push(DefinitionPathSegment {
        kind: DefinitionKind::Method,
        name: "head".into(),
        occurrence: 0,
    });
    let resolved = vm
        .runtime()
        .resolve_interface_method(&value, &method)
        .unwrap();
    let old_key = resolved.implementation().key();
    assert!(matches!(resolved.target(), CallableTarget::Native(_)));
    let new = vm
        .reload_artifact(&old, "associated-reload", artifact, &Default::default())
        .unwrap();
    vm.runtime().collect_garbage().unwrap();
    let result = vm.invoke_interface_method(&value, &method, &[]).unwrap();
    let Value::Array(id) = result else {
        panic!("associated array result")
    };
    assert_eq!(vm.runtime().gc().array_get(id, 0), Some(Value::I32(42)));
    assert_eq!(
        vm.runtime()
            .resolve_interface_method(&value, &method)
            .unwrap()
            .implementation()
            .key(),
        old_key
    );
    let fresh = vm.execute(&new, "dynamic_boxed").unwrap().return_value;
    let fresh_method = vm
        .runtime()
        .resolve_interface_method(&fresh, &method)
        .unwrap();
    assert_ne!(fresh_method.implementation().key(), old_key);
    assert!(
        vm.invoke_interface_method(&value, &method, &[Value::Bool(true)])
            .is_err()
    );
    assert_eq!(calls.get(), 1);
    assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
    drop(fresh_method);
    drop(resolved);
    drop(root);
    vm.runtime().collect_garbage().unwrap();
    assert_eq!(vm.runtime().gc().active_roots(), 0);
    assert_eq!(vm.runtime().gc().allocated_objects(), 0);
}

#[test]
fn forged_native_interface_slots_are_rejected_before_linking_or_factory_entry() {
    for case in 0..5 {
        let mut program = KbcArtifact::from_bytes(ARTIFACT).unwrap().program;
        let module = program
            .modules
            .iter_mut()
            .find(|module| {
                module.interface_tables.iter().any(|table| {
                    table
                        .methods
                        .iter()
                        .any(|slot| matches!(slot.target, CallableTarget::Native(_)))
                })
            })
            .unwrap();
        let table = module
            .interface_tables
            .iter_mut()
            .find(|table| {
                table
                    .methods
                    .iter()
                    .any(|slot| matches!(slot.target, CallableTarget::Native(_)))
            })
            .unwrap();
        match case {
            0 => table.methods.clear(),
            1 => {
                table.methods[0].target =
                    CallableTarget::Native(NativeImportId::new(u32::MAX as usize))
            }
            2 => {
                let CallableTarget::Native(import) = table.methods[0].target else {
                    unreachable!()
                };
                module.native_imports[import.index()].signature.result =
                    AbiType::Builtin(BuiltinType::Bool);
            }
            3 => table.methods.push(table.methods[0].clone()),
            4 => table.methods[0].target = CallableTarget::Script(Default::default()),
            _ => unreachable!(),
        }
        assert!(
            KbcArtifact::from_program(program, Default::default()).is_err(),
            "forged native slot {case}"
        );
    }
}

#[test]
fn forged_associated_import_results_and_bindings_are_rejected_offline() {
    for case in 0..2 {
        let mut program = KbcArtifact::from_bytes(ARTIFACT).unwrap().program;
        if case == 0 {
            let import = program
                .modules
                .iter_mut()
                .flat_map(|module| &mut module.native_imports)
                .find(|import| {
                    import.binding.module.package.0 == "game"
                        && import.binding.path.last().unwrap().name == "echo"
                })
                .unwrap();
            import.signature.result = AbiType::Builtin(BuiltinType::Bool);
        } else {
            let table = program
                .modules
                .iter_mut()
                .flat_map(|module| &mut module.public_items)
                .find_map(|item| {
                    if let kagari_abi::types::PublicAbiItem::InterfaceTable(table) = item
                        && table.declaration.module.package.0 == "game"
                    {
                        Some(table)
                    } else {
                        None
                    }
                })
                .unwrap();
            let AbiType::Trait(interface) = &mut table.trait_type else {
                unreachable!()
            };
            *interface.associated_types.values_mut().next().unwrap() =
                AbiType::Builtin(BuiltinType::Bool);
        }
        assert!(KbcArtifact::from_program(program, Default::default()).is_err());
    }
}

#[cfg(feature = "source")]
mod source {
    use super::*;
    use kagari_common::{source::SourceFile, source_database::SourceLayer};

    #[test]
    fn registered_associated_members_navigate_to_generated_docs() {
        let engine = engine(Rc::new(Cell::new(0)));
        for text in [
            "use game::associated::Source; fn read<S: Source>(source: S) -> S::Item { source.head() }",
            "use game::associated::Source; fn read<S: Source<Item = i32>>(source: S) -> S::Item { source.head() }",
            "use game::associated::Source; fn read<S: Source>(source: S) -> <S as Source>::Item { source.head() }",
        ] {
            let file = engine
                .set_source(
                    "memory://native-associated-tooling.kgr",
                    text.into(),
                    SourceLayer::Base,
                )
                .unwrap();
            let snapshot = engine
                .analyze(
                    engine.source_snapshot(),
                    Default::default(),
                    &Default::default(),
                )
                .unwrap();
            let offset = text.find("::Item").unwrap() + 2;
            let declaration = snapshot.definition_at(file, offset).unwrap();
            let source = snapshot.source(declaration.location.file).unwrap();
            assert_eq!(source.name(), "kagari://native/game/associated.kgr");
            assert_eq!(
                &source.text()[declaration.location.range.start..declaration.location.range.end],
                "Item"
            );
            assert_eq!(
                snapshot
                    .documentation_at(file, offset)
                    .unwrap()
                    .documentation,
                "The value produced by this source."
            );
        }
    }

    #[test]
    fn source_emission_matches_the_associated_fixture() {
        let engine = engine(Rc::new(Cell::new(0)));
        let artifact = engine
            .compile_to_artifact(
                SourceFile::new(
                    "memory://native-associated.kgr",
                    include_str!("fixtures/native_associated.kgr"),
                ),
                Default::default(),
                Default::default(),
            )
            .unwrap();
        assert_eq!(artifact.to_bytes().unwrap(), ARTIFACT);
    }

    #[test]
    fn registered_output_bounds_reject_missing_and_wrong_script_associations() {
        let calls = Rc::new(Cell::new(0));
        let engine = engine(calls.clone());
        for text in [
            "use game::associated::Marked; struct Owner {} impl Marked for Owner {} fn main() {}",
            "use game::associated::Marked; struct Owner {} impl Marked for Owner { type Item = i32; } fn main() {}",
            "use game::associated::{echo, Bag}; fn main() { echo::<Bag<i32>>(true, [1]); }",
        ] {
            assert!(
                engine
                    .compile_source(
                        SourceFile::new("memory://invalid-associated-native.kgr", text),
                        Default::default()
                    )
                    .is_err()
            );
        }
        let valid = "use game::associated::{Marker, Marked}; struct Token {} struct Owner {} impl Marker for Token {} impl Marked for Owner { type Item = Token; } fn main() -> i32 { 42 }";
        assert!(
            engine
                .compile_source(
                    SourceFile::new("memory://valid-associated-native.kgr", valid),
                    Default::default()
                )
                .is_ok()
        );
        assert_eq!(calls.get(), 0);
    }
}

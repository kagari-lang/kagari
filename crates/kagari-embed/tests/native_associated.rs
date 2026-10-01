//! Registered ordinary associated types use the shared static and offline contracts.
// Test/cross-target sharing keeps the fixture's application registration authoritative.
#[path = "fixtures/native_associated_api.rs"]
mod fixture_api;
use kagari_abi::{
    callable::CallableImplementation,
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
use kagari_runtime::{
    Runtime, RuntimeConfig,
    error::RuntimeError,
    native::{
        NativeAction, NativeContext, NativeInvocationState,
        api::{NativeApi, NativeHandler},
        cmp_api::cmp,
        packages::standard_library,
    },
    value::Value,
};
use kagari_vm::vm::Vm;
use std::{cell::Cell, rc::Rc};

const ARTIFACT: &[u8] = include_bytes!("fixtures/native_associated.kbc");
fn engine(calls: Rc<Cell<usize>>) -> KagariEngine {
    configured_engine(calls, true)
}
fn configured_engine(calls: Rc<Cell<usize>>, defaults: bool) -> KagariEngine {
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let mut builder = KagariEngine::builder()
        .config(config)
        .install_standard_library(defaults)
        .install(Ok(fixture_api::api(fixture_api::module(), calls)))
        .install(fixture_api::typed::native_api());
    // This product was emitted with the default prelude's cmp dependency.
    if !defaults {
        builder = builder.install(cmp::native_api());
    }
    builder.build().unwrap()
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
        "impl<T0> Source for Bag<T0> where T0: Hook {\n    type Item = T0;\n    pub fn head(self) -> T0;"
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
fn selected_trait_callbacks_use_concrete_native_or_private_script_targets() {
    let calls = Rc::new(Cell::new(0));
    let engine = configured_engine(calls.clone(), false);
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime
        .load_program(&prepared(), Default::default())
        .unwrap();
    for name in ["selected_native_main", "selected_script_main"] {
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
        .execute(&loaded, "selected_native_empty", &[], &context)
        .unwrap_err();
    assert_eq!(error.error_trace().unwrap().frames[0].function_name, "head");
    assert_eq!(calls.get(), 2);
    assert_eq!(runtime.runtime().gc().active_roots(), 0);
    assert_eq!(
        runtime.runtime().resources().counters().current_call_depth,
        0
    );
}

struct InvalidSelected(u8);
impl NativeInvocationState for InvalidSelected {
    fn advance(&mut self, context: &mut NativeContext<'_>) -> Result<NativeAction, RuntimeError> {
        let (slot, arguments) = match self.0 {
            0 => (1, vec![context.argument(0).unwrap()]),
            1 => (0, vec![]),
            2 => (0, vec![Value::Bool(true)]),
            _ => unreachable!(),
        };
        context
            .selected_callback(slot, arguments)
            .map(NativeAction::Callback)
    }
}

#[test]
fn selected_callbacks_reject_invalid_slots_and_arguments_before_target_entry() {
    for case in 0..3 {
        let target_calls = Rc::new(Cell::new(0));
        let module = fixture_api::module();
        let handlers = [
            "head",
            "echo",
            "forward",
            "selected_head",
            "nested_head",
            "check_first",
        ]
        .into_iter()
        .map(|name| {
            let target_calls = target_calls.clone();
            NativeHandler::new(binding_id(&module.identity, name), 0, move |_| {
                if name == "head" {
                    target_calls.set(target_calls.get() + 1);
                }
                Ok(Box::new(InvalidSelected(case)))
            })
        })
        .collect();
        let engine = KagariEngine::builder()
            .install_standard_library(false)
            .install(cmp::native_api())
            .install(NativeApi::new(vec![module], handlers, Default::default()))
            .install(fixture_api::typed::native_api())
            .build()
            .unwrap();
        let context = ExecutionContext::default();
        let mut runtime = engine.runtime(context.clone());
        let loaded = runtime
            .load_program(&prepared(), Default::default())
            .unwrap();
        let error = runtime
            .execute(&loaded, "selected_native_main", &[], &context)
            .unwrap_err();
        assert_eq!(error.code(), "KG_BYTECODE_VERIFICATION_FAILED");
        assert_eq!(
            error.error_trace().unwrap().frames[0].function_name,
            "selected_native_main"
        );
        assert_eq!(target_calls.get(), 0);
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
fn forged_selected_trait_dependencies_are_rejected_offline() {
    for case in 0..9 {
        let mut program = KbcArtifact::from_bytes(ARTIFACT).unwrap().program;
        let import = program
            .modules
            .iter_mut()
            .flat_map(|module| &mut module.native_imports)
            .find(|import| {
                import.callables.first().is_some_and(|call| {
                    matches!(call.implementation, CallableImplementation::Native(_))
                        && !call.instance.arguments.is_empty()
                })
            })
            .unwrap();
        match case {
            0 => import.callables.clear(),
            1 => import.callables[0].signature.result = AbiType::Builtin(BuiltinType::Bool),
            2 => import.callables[0].requirement.receiver = AbiType::Builtin(BuiltinType::Bool),
            3 => import.callables[0].instance.arguments.clear(),
            4 => import.callables[0].effects.writes_aggregate = false,
            5 => {
                import.callables[0]
                    .instance
                    .declaration
                    .path
                    .last_mut()
                    .unwrap()
                    .name = "missing".into()
            }
            6 => import.callables.push(import.callables[0].clone()),
            7 => import.callables[0].implementation = CallableImplementation::Script,
            8 => {
                import.callables[0]
                    .requirement
                    .member
                    .path
                    .last_mut()
                    .unwrap()
                    .name = "missing".into()
            }
            _ => unreachable!(),
        }
        assert!(
            KbcArtifact::from_program(program, Default::default()).is_err(),
            "forged selection {case}"
        );
    }
}

#[test]
fn installed_native_templates_must_match_the_declared_callable_dependencies() {
    let calls = Rc::new(Cell::new(0));
    let mut module = fixture_api::module();
    module.callable_requirements.clear();
    let engine = KagariEngine::builder()
        .install(Ok(fixture_api::api(module, calls.clone())))
        .install(fixture_api::typed::native_api())
        .build()
        .unwrap();
    let mut runtime = engine.runtime(Default::default());
    assert!(
        runtime
            .load_program(&prepared(), Default::default())
            .is_err()
    );
    assert_eq!(calls.get(), 0);
}

#[test]
fn native_callable_requirements_validate_members_binders_and_declared_bounds() {
    for case in 0..5 {
        let mut module = fixture_api::module();
        let owner = module.definition(DefinitionKind::Function, "selected_head");
        let requirement = module
            .callable_requirements
            .get_mut(&owner)
            .unwrap()
            .first_mut()
            .unwrap();
        match case {
            0 => requirement.member.path.last_mut().unwrap().name = "missing".into(),
            1 => {
                requirement.receiver = AbiType::Parameter {
                    owner: requirement.member.clone(),
                    position: 0,
                }
            }
            2 => requirement
                .arguments
                .push(AbiType::Builtin(BuiltinType::I32)),
            3 => {
                requirement
                    .interface
                    .declaration
                    .path
                    .last_mut()
                    .unwrap()
                    .name = "missing".into()
            }
            4 => module
                .functions
                .iter_mut()
                .find(|function| function.name == "selected_head")
                .unwrap()
                .bounds
                .clear(),
            _ => unreachable!(),
        }
        assert!(
            module.validate().is_err(),
            "invalid declared callable {case}"
        );
    }
}

#[test]
fn registration_rejects_malformed_impl_and_inherent_method_bounds() {
    for case in 0..5 {
        let mut module = fixture_api::module();
        match case {
            0 => {
                module.implementations[0].bounds[0].ty = AbiType::Parameter {
                    owner: module.implementation_id(1),
                    position: 0,
                }
            }
            1 => {
                let bound = &mut module.implementations[0].bounds[0];
                bound.constraints.push(bound.constraints[0].clone());
            }
            2 => {
                let method = &mut module.implementations[1].methods[0];
                method.bounds.push(method.bounds[0].clone());
            }
            3 => {
                let bound = &mut module.implementations[1].methods[0].bounds[0];
                bound.constraints.push(bound.constraints[0].clone());
            }
            4 => {
                module.implementations[1].methods[0].bounds[0].ty = AbiType::Parameter {
                    owner: module.implementation_id(0),
                    position: 0,
                }
            }
            _ => unreachable!(),
        }
        assert!(module.validate().is_err(), "malformed native bounds {case}");
    }
}

#[test]
fn associated_output_receivers_select_their_own_native_member_instances() {
    let calls = Rc::new(Cell::new(0));
    let engine = configured_engine(calls.clone(), false);
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime
        .load_program(&prepared(), Default::default())
        .unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, "selected_nested_main", &[], &context)
            .unwrap()
            .return_value,
        Value::I32(42)
    );
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
fn native_inherent_method_requirements_use_checked_script_callbacks_offline() {
    let engine = configured_engine(Rc::new(Cell::new(0)), false);
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime
        .load_program(&prepared(), Default::default())
        .unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, "selected_method_main", &[], &context)
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
    for entry in [
        "callback_native_main",
        "selected_native_main",
        "selected_script_main",
        "selected_nested_main",
        "selected_method_main",
    ] {
        let mut finished = false;
        for limit in 0..100 {
            let mut limited = context.clone();
            limited.resources.max_instruction_steps = Some(limit);
            let before = calls.get();
            match runtime.execute(&loaded, entry, &[], &limited) {
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
        assert!(finished, "budget sweep must reach completion for {entry}");
    }
    assert!(entered_before_exhaustion);
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

    #[cfg(feature = "native")]
    #[test]
    fn portable_mir_rejects_missing_or_forged_native_method_instances() {
        use kagari_mir::{codec::decode_program, program::verify_program};
        let engine = engine(Rc::new(Cell::new(0)));
        let artifact = engine
            .compile_to_artifact(
                SourceFile::new(
                    "memory://native-method-mir.kgr",
                    "
            use game::associated::{Source, Hook};
            impl Hook for i32 { fn check(self) {} }
            fn main() -> i32 { val source: Source<Item = i32> = [42]; source.head() }
        ",
                ),
                Default::default(),
                Default::default(),
            )
            .unwrap();
        let payload = &artifact.portable_mir.as_ref().unwrap().bytes;
        let checked = decode_program(payload, &Default::default()).unwrap();
        let root = checked.root().clone();
        let modules = checked.into_unverified();
        for case in 0..5 {
            let mut forged = modules.clone();
            let owner = forged
                .iter_mut()
                .find(|module| !module.native_targets.is_empty())
                .unwrap();
            match case {
                0 => owner.native_targets.clear(),
                1 => owner.native_targets.push(owner.native_targets[0].clone()),
                2 => owner.native_targets[0].callables.clear(),
                3 => owner.native_targets[0].signature.result = AbiType::Builtin(BuiltinType::Bool),
                4 => owner.native_targets[0].instance.declaration.module = root.clone(),
                _ => unreachable!(),
            }
            assert!(
                verify_program(root.clone(), forged, &Default::default()).is_err(),
                "forged native MIR {case}"
            );
        }
    }

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
    fn selected_private_script_targets_remain_pinned_after_reload() {
        let engine = engine(Rc::new(Cell::new(0)));
        let source = include_str!("fixtures/native_associated.kgr").replace(
            "fn head(self) -> ArrayList<i32> { self.values }",
            "fn head(self) -> ArrayList<i32> { [self.values[0usize] + 1] }",
        );
        let artifact = engine
            .compile_to_artifact(
                SourceFile::new("memory://native-associated.kgr", source),
                Default::default(),
                Default::default(),
            )
            .unwrap();
        let candidate =
            PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap();
        let context = ExecutionContext::default();
        let mut runtime = engine.runtime(context.clone());
        let old = runtime
            .load_program(&prepared(), Default::default())
            .unwrap();
        let current = runtime
            .reload_program(&old, &candidate, Default::default())
            .unwrap();
        runtime.runtime().collect_garbage().unwrap();
        for (program, expected) in [(&old, 42), (&current, 43)] {
            assert_eq!(
                runtime
                    .execute(program, "selected_script_main", &[], &context)
                    .unwrap()
                    .return_value,
                Value::I32(expected)
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
    fn native_method_script_callback_traps_release_retained_values_and_frames() {
        let calls = Rc::new(Cell::new(0));
        let engine = engine(calls.clone());
        let text = include_str!("fixtures/native_associated.kgr")
            .replace("scratch[0usize];", "scratch[1usize];");
        let artifact = engine
            .compile_to_artifact(
                SourceFile::new("memory://native-method-trap.kgr", text),
                Default::default(),
                Default::default(),
            )
            .unwrap();
        let program =
            PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap();
        let context = ExecutionContext::default();
        let mut runtime = engine.runtime(context.clone());
        let loaded = runtime.load_program(&program, Default::default()).unwrap();
        for entry in [
            "selected_method_main",
            "dynamic_main",
            "selected_native_main",
        ] {
            let error = runtime.execute(&loaded, entry, &[], &context).unwrap_err();
            assert_eq!(
                error.error_trace().unwrap().frames[0].function_name,
                "check"
            );
            assert_eq!(runtime.runtime().gc().active_roots(), 0);
            assert_eq!(
                runtime.runtime().resources().counters().current_call_depth,
                0
            );
            assert!(!runtime.runtime().is_quarantined());
            runtime.runtime().collect_garbage().unwrap();
            assert_eq!(runtime.runtime().gc().allocated_objects(), 0);
        }
        assert_eq!(calls.get(), 2);
    }

    #[test]
    fn registered_output_bounds_reject_missing_and_wrong_script_associations() {
        let calls = Rc::new(Cell::new(0));
        let engine = engine(calls.clone());
        for text in [
            "use game::associated::Marked; struct Owner {} impl Marked for Owner {} fn main() {}",
            "use game::associated::Marked; struct Owner {} impl Marked for Owner { type Item = i32; } fn main() {}",
            "use game::associated::{echo, Bag}; fn main() { echo::<Bag<i32>>(true, [1]); }",
            "fn main() { [true].check_first(); }",
            "use game::associated::Source; fn main() { val source: Source<Item = bool> = [true]; source.head(); }",
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

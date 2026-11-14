use crate::{
    Runtime,
    closure::{ClosureMode, ClosureRecord},
    error::{RuntimeError, RuntimeErrorKind, trap::TrapReason},
    execution::roots::{ExecutionRoots, ValueBank},
    frame::{arguments::FrameArguments, types::EnvironmentRecord},
    module::{ExecutableModule, dispatch::CallRouteKind, managed::source::RevisionSource},
    native::conversion::{arguments::ArgumentValues, context::ConversionContext},
    test_support::update_programs as programs,
    value::{carriers::ScopedValue, closures::ClosureObject, strings::StringObject},
};
use kagari_bytecode::{instruction::Slot, module::CallableTarget};
use kagari_contract::ids::FunctionRef;
use kagari_gc::error::GcError;
use kagari_types::{scalar::BuiltinType, ty::Ty};
use std::sync::Arc;

fn captured_function(owner: &ExecutableModule) -> FunctionRef {
    owner
        .bytecode
        .functions
        .iter()
        .find(|function| {
            function.metadata.params.len() == 1
                && owner
                    .bind_call_route(CallableTarget::Script(function.id))
                    .unwrap()
                    .kind()
                    == CallRouteKind::BodyOwned
        })
        .expect("verified generated closure with one String capture")
        .id
}

#[test]
fn retained_host_closure_admission_releases_the_object_after_frame_copy() {
    let source = include_str!("../../../tests/fixtures/metadata/closure.kgr");
    let mut runtime = Runtime::default();
    let program = runtime
        .load_verified_program("host-closure", programs::source_code(source))
        .unwrap();
    let owner = runtime.select_program(&program).unwrap();
    let function = captured_function(&owner);
    runtime.collect_garbage().unwrap();
    let baseline = runtime.gc_stats();
    let text = runtime
        .memory
        .scope(&runtime.resources, |cx, _| {
            let text = StringObject::alloc(cx, "captured by frame".into())?;
            ScopedValue::managed(text).retain(cx)
        })
        .unwrap();
    let root = {
        let mut conversion = ConversionContext::new(&mut runtime, &owner).unwrap();
        let text = conversion.import(&text).unwrap();
        let closure = conversion
            .allocate_script_closure(function, &[text], None, ClosureMode::Retained)
            .unwrap();
        conversion.retain(&closure).unwrap()
    };
    drop(text);
    let mut activation = runtime.enter_execution_stack(&owner).unwrap();
    activation
        .with(|runtime, stack| {
            let execution = runtime.resources().active_session().unwrap().id;
            let before = runtime
                .memory
                .scope(&runtime.resources, |cx, roots| {
                    ExecutionRoots::with(roots, cx, |slots| slots.len(execution, ValueBank::Local))
                })
                .unwrap();
            stack.push_rooted_closure(runtime, &root, ArgumentValues::scalars(&[]))?;
            let after = runtime
                .memory
                .scope(&runtime.resources, |cx, roots| {
                    ExecutionRoots::with(roots, cx, |slots| slots.len(execution, ValueBank::Local))
                })
                .unwrap();
            assert_eq!(
                after, before,
                "the frame borrows the root directly instead of retaining a local copy"
            );
            Ok(())
        })
        .unwrap();
    drop(root);
    activation
        .with(|runtime, stack| {
            runtime.collect_garbage().unwrap();
            assert_eq!(
                runtime.gc_stats().objects,
                baseline.objects + 1,
                "only the captured text remains, not its retired closure object"
            );
            assert_eq!(
                runtime.gc_stats().root_registrations,
                baseline.root_registrations
            );
            let callee = owner
                .execution()
                .callee(CallableTarget::Script(function))
                .unwrap();
            let location = {
                let frame = stack.current(runtime)?;
                let register = Slot::new(callee.parameter_slots[0]);
                frame.read_slot(runtime, register)?.location().unwrap()
            };
            runtime
                .memory
                .scope(&runtime.resources, |cx, roots| {
                    ExecutionRoots::with(roots, cx, |source| {
                        let value = source.get(location)?;
                        StringObject::read(source.scope(), value, |text| {
                            assert_eq!(text, "captured by frame")
                        })
                    })
                })
                .unwrap();
            Ok(())
        })
        .unwrap();
    drop(activation);
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, baseline.objects);
    assert_eq!(
        runtime.gc_stats().root_registrations,
        baseline.root_registrations
    );
}

#[test]
fn captured_values_and_exact_body_survive_independent_sources_without_host_roots() {
    let source = include_str!("../../../tests/fixtures/metadata/closure.kgr");
    let mut runtime = Runtime::default();
    let program = runtime
        .load_verified_program("closure", programs::source_code(source))
        .unwrap();
    let owner = runtime.select_program(&program).unwrap();
    let function = captured_function(&owner);
    let record = Arc::new(
        ClosureRecord::script(&runtime, &owner, function, 1, None, ClosureMode::Ordinary).unwrap(),
    );
    assert!(record.call_route().is_none());
    assert!(record.signature().params.is_empty());
    assert_eq!(
        record.signature().result.ty(),
        &Ty::Builtin(BuiltinType::String)
    );
    runtime.collect_garbage().unwrap();
    let baseline = runtime.gc_stats();
    let original = runtime.memory.enter_execution(&runtime.resources).unwrap();
    let location = runtime
        .with_revision_origin(&owner, RevisionSource::Current, |slots, body| {
            let captured = StringObject::alloc(slots.scope(), "retained capture".into())?;
            let object = ClosureObject::alloc(
                slots.scope(),
                record.clone(),
                body,
                None,
                &[ScopedValue::managed(captured)],
                Default::default(),
            )?;
            slots.push_local(original, ScopedValue::managed(object))
        })
        .unwrap();
    let original_view = runtime.memory.inspect_value(location).unwrap();
    let survivor = runtime.memory.enter_execution(&runtime.resources).unwrap();
    let copied = runtime
        .memory
        .scope(&runtime.resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                let value = slots.get(location)?;
                slots.push_local(survivor, value)
            })
        })
        .unwrap();
    runtime
        .memory
        .retire_execution(&runtime.resources, original)
        .unwrap();
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, baseline.objects + 2);
    assert_eq!(
        runtime.gc_stats().root_registrations,
        baseline.root_registrations
    );
    assert_eq!(
        runtime.resolve_closure(&original_view).unwrap_err(),
        RuntimeError::from(TrapReason::InvalidClosureHandle)
    );
    let current_view = runtime.memory.inspect_value(copied).unwrap();
    let observed = runtime.resolve_closure(&current_view).unwrap();
    assert!(Arc::ptr_eq(&observed, &record));
    let (captured, body) = runtime
        .memory
        .scope(&runtime.resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                let value = slots.get(copied)?;
                let object = slots
                    .scope()
                    .downcast::<ClosureObject>(value.object().ok_or(GcError::Representation)?)?;
                let captured = ClosureObject::capture(slots.scope(), object, 0)?;
                let captured = slots
                    .scope()
                    .downcast::<StringObject>(captured.object().ok_or(GcError::Representation)?)?;
                let captured = slots
                    .scope()
                    .read(captured, |captured| captured.as_str().to_owned());
                let body =
                    ClosureObject::body(slots.scope(), object).expect("exact generated body");
                let body = slots
                    .scope()
                    .read(body, |body| body.code().program_identity());
                Ok((captured, body))
            })
        })
        .unwrap();
    assert_eq!(captured, "retained capture");
    assert_eq!(body, owner.program_identity());
    let named_function = owner
        .bytecode
        .functions
        .iter()
        .find(|function| function.name == "make")
        .unwrap()
        .id;
    let named_record = Arc::new(
        ClosureRecord::script(
            &runtime,
            &owner,
            named_function,
            0,
            None,
            ClosureMode::Ordinary,
        )
        .unwrap(),
    );
    assert!(named_record.call_route().is_some());
    let named_source = runtime.memory.enter_execution(&runtime.resources).unwrap();
    let named_location = runtime
        .with_revision_origin(&owner, RevisionSource::Current, |slots, body| {
            let object = ClosureObject::alloc(
                slots.scope(),
                named_record.clone(),
                body,
                None,
                &[],
                Default::default(),
            )?;
            slots.push_local(named_source, ScopedValue::managed(object))
        })
        .unwrap();
    let staged = runtime
        .stage_reload_program(&program, None, programs::bytecode(source, Some(&owner)))
        .unwrap();
    runtime.publish_staged_reload(staged).unwrap();
    let current = runtime.select_program(&program).unwrap();
    assert_ne!(current.publication_token(), owner.publication_token());
    runtime.collect_garbage().unwrap();
    assert!(
        runtime.modules.contains_module(&owner).unwrap(),
        "exact closure retains old mutable body state"
    );
    assert_eq!(
        observed.observe(&runtime).unwrap().publication,
        owner.publication_token()
    );
    assert_eq!(
        named_record.observe(&runtime).unwrap().publication,
        current.publication_token()
    );
    runtime
        .memory
        .retire_execution(&runtime.resources, survivor)
        .unwrap();
    runtime.collect_garbage().unwrap();
    assert!(
        !runtime.modules.contains_module(&owner).unwrap(),
        "a named relationship and pure records do not retain the old body"
    );
    let named_view = runtime.memory.inspect_value(named_location).unwrap();
    let named = runtime.resolve_closure(&named_view).unwrap();
    assert_eq!(
        named.observe(&runtime).unwrap().publication,
        current.publication_token()
    );
    runtime
        .memory
        .retire_execution(&runtime.resources, named_source)
        .unwrap();
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, baseline.objects);
    assert_eq!(
        runtime.gc_stats().root_registrations,
        baseline.root_registrations
    );
    assert_eq!(
        runtime.resolve_closure(&current_view).unwrap_err(),
        RuntimeError::from(TrapReason::InvalidClosureHandle)
    );
    // Even both surviving Arc records are facts rather than graph roots.
    assert_eq!(
        observed.signature().result.ty(),
        record.signature().result.ty()
    );
    assert!(!runtime.is_quarantined());
}

#[test]
fn named_generic_selection_transports_live_environment_after_old_body_reclamation() {
    let source = include_str!("../../../tests/fixtures/metadata/applications.kgr");
    let mut runtime = Runtime::default();
    let installed = runtime
        .load_verified_program("named-generic", programs::source_code(source))
        .unwrap();
    let old = runtime.select_program(&installed).unwrap();
    let function = old
        .bytecode
        .functions
        .iter()
        .find(|function| {
            function
                .metadata
                .semantic
                .generic
                .as_ref()
                .is_some_and(|generic| {
                    generic.parameters.first().is_some_and(|parameter| {
                        old.definition_name(parameter.owner) == Some("relay")
                    })
                })
        })
        .expect("checked shared relay body");
    let generic = function.metadata.semantic.generic.as_ref().unwrap();
    let execution = runtime.memory.enter_execution(&runtime.resources).unwrap();
    let arguments = runtime
        .resolve_type_arguments(&old, &[Ty::Builtin(BuiltinType::I32)])
        .unwrap();
    let environment = runtime
        .alloc_environment(
            execution,
            EnvironmentRecord::new(old.definitions(), generic.parameters.clone(), arguments)
                .unwrap(),
            &[],
        )
        .unwrap();
    let record = Arc::new(
        ClosureRecord::script(
            &runtime,
            &old,
            function.id,
            0,
            Some(environment.facts().clone()),
            ClosureMode::Ordinary,
        )
        .unwrap(),
    );
    assert!(record.call_route().is_some());
    let location = runtime
        .with_revision_origin(&old, RevisionSource::Current, |slots, body| {
            let environment = slots.dependency(environment.source())?;
            let object = ClosureObject::alloc(
                slots.scope(),
                record.clone(),
                body,
                Some(environment),
                &[],
                Default::default(),
            )?;
            slots.push_local(execution, ScopedValue::managed(object))
        })
        .unwrap();
    let value = runtime.memory.inspect_value(location).unwrap();
    let staged = runtime
        .stage_reload_program(&installed, None, programs::bytecode(source, Some(&old)))
        .unwrap();
    runtime.publish_staged_reload(staged).unwrap();
    runtime.collect_garbage().unwrap();
    assert!(
        !runtime.modules.contains_module(&old).unwrap(),
        "scalar environment facts and a named route do not retain an old mutable revision"
    );
    let current = runtime.select_program(&installed).unwrap();
    let selected = runtime.select_closure_call(execution, &value).unwrap();
    assert_eq!(
        selected.owner().program_identity(),
        current.program_identity()
    );
    let (facts, source) = selected.environment().unwrap();
    assert_ne!(facts.id, environment.facts().id);
    let target = match selected.target() {
        CallableTarget::Script(function) => function,
        _ => panic!("generic script target"),
    };
    let generic = current.bytecode.functions[target.index()]
        .metadata
        .semantic
        .generic
        .as_ref()
        .unwrap();
    assert!(facts.types.matches(generic));
    assert_eq!(
        facts
            .types
            .argument(&generic.parameters[0].owner, 0)
            .unwrap()
            .ty(),
        &Ty::Builtin(BuiltinType::I32)
    );
    runtime.validate_environment_in(&[source], &facts).unwrap();
    let roots = runtime.gc_stats().root_registrations;
    let frozen = {
        let mut conversion = ConversionContext::for_selected(
            &mut runtime,
            selected.owner(),
            execution,
            selected.origin(),
            Some(source),
        )
        .unwrap();
        let frozen = conversion.freeze_closure(&selected).unwrap();
        conversion.retain(&frozen).unwrap()
    };
    runtime
        .memory
        .retire_execution(&runtime.resources, execution)
        .unwrap();
    runtime.collect_garbage().unwrap();
    let empty_captures = FrameArguments::pack(ArgumentValues::scalars(&[]))
        .with_captures(value.reborrow(), 0)
        .unwrap();
    assert!(
        matches!(
            runtime.memory.scope(&runtime.resources, |cx, roots| {
                ExecutionRoots::with(roots, cx, |slots| empty_captures.check_bounds(slots))
            }),
            Err(GcError::Index)
        ),
        "an empty capture prefix must still admit its original source"
    );
    assert_eq!(runtime.gc_stats().root_registrations, roots + 1);
    assert_eq!(
        runtime
            .validate_environment_in(&[source], &facts)
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ModuleValidation
    );
    assert_eq!(
        runtime
            .observe_callable_value(&frozen)
            .unwrap()
            .unwrap()
            .publication,
        current.publication_token()
    );
    drop(frozen);
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().root_registrations, roots);
    assert!(!runtime.is_quarantined());
}

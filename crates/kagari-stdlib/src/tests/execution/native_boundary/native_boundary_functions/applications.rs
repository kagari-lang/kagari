use super::{compile_without_foundation, fixture, fixture_without_foundation};
use crate::{
    declarations::StandardDeclarations,
    tests::{
        execution::native_boundary::{
            native_boundary_interfaces::generic_identity_module, support::declaration_query,
        },
        support::bytecode::reload_bytecode,
    },
};

use kagari_runtime::{
    error::RuntimeErrorKind,
    native::{
        binding::{Codec, NativeBinding},
        builder::ModuleBuilder,
        conversion::context::ConversionContext,
        declarations::{CallableRequirement, FunctionDecl, MethodDecl},
        function_handle::Function,
        objects::Object,
        types::Type,
    },
    value::view::ValueView,
};

use kagari_common::identity::{DefinitionKind, DefinitionPath, DefinitionPathSegment};

use std::{
    slice,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

#[test]
fn generic_function_binding_uses_closed_witnesses_and_rejects_missing_applications() {
    let module = generic_identity_module();
    let declaration = declaration_query(&module, DefinitionKind::Function, "identity");
    let (mut vm, owner) = fixture(
        include_str!("../../../../../tests/fixtures/native/function-generic-witnesses.kgr"),
        Some(&module),
    );
    let _owner_program = owner;
    let owner = vm.select_program(&_owner_program).unwrap();
    let conversion = ConversionContext::new(&mut vm, &owner).unwrap();
    let i32_type = conversion.type_for::<i32>().unwrap();
    let string_type = conversion.type_for::<String>().unwrap();
    drop(conversion);
    let identity = vm
        .bind_function_application_declaration::<(i32,), i32>(
            &_owner_program.root(),
            &declaration,
            slice::from_ref(&i32_type),
        )
        .unwrap();
    assert_eq!(vm.call(&identity, (42,)).unwrap(), 42);
    assert!(
        vm.bind_function::<(i32,), i32>(&_owner_program.root(), "identity")
            .is_err()
    );
    let hidden = DefinitionPath {
        module: owner.bytecode.identity.clone(),
        path: vec![DefinitionPathSegment {
            kind: DefinitionKind::Function,
            name: "hidden".into(),
            occurrence: 0,
        }],
    };

    assert!(
        vm.bind_function_application_declaration::<(String,), String>(
            &_owner_program.root(),
            &declaration,
            &[string_type]
        )
        .is_err()
    );
    assert!(
        vm.bind_function_application_declaration::<(i32,), i32>(
            &_owner_program.root(),
            &hidden,
            slice::from_ref(&i32_type)
        )
        .is_err()
    );
    assert!(
        vm.bind_function_application_declaration::<(i32,), i32>(
            &_owner_program.root(),
            &declaration,
            &[]
        )
        .is_err()
    );
    assert!(
        vm.bind_function_application_declaration::<(i32,), bool>(
            &_owner_program.root(),
            &declaration,
            &[i32_type]
        )
        .is_err()
    );
}

#[test]
fn generic_native_calls_return_retained_objects_tuples_and_closures() {
    let module = generic_identity_module();
    let identity_id = declaration_query(&module, DefinitionKind::Function, "identity");
    let singleton_id = declaration_query(&module, DefinitionKind::Function, "singleton");
    let (mut vm, owner) = fixture(
        include_str!("../../../../../tests/fixtures/native/function-generic-retained-values.kgr"),
        Some(&module),
    );
    let _owner_program = owner;
    let owner = vm.select_program(&_owner_program).unwrap();
    let conversion = ConversionContext::new(&mut vm, &owner).unwrap();
    let i32_type = conversion.type_for::<i32>().unwrap();
    type Callback = Function<(), i32>;
    let callback_type = conversion.type_for::<Callback>().unwrap();
    drop(conversion);
    let item = vm.bind_type(&_owner_program.root(), "Item", &[]).unwrap();
    let identity = vm
        .bind_function_application_declaration::<(Object,), Object>(
            &_owner_program.root(),
            &identity_id,
            slice::from_ref(item.type_argument()),
        )
        .unwrap();
    let make = vm
        .bind_function::<(), Object>(&_owner_program.root(), "make")
        .unwrap();
    let made = vm.call(&make, ()).unwrap();
    let object = vm.call(&identity, (made,)).unwrap();
    vm.collect_garbage().unwrap();
    let value = vm.bind_field::<i32>(&item, "value").unwrap();
    assert_eq!(
        object
            .get(&mut vm.context(&owner).unwrap(), &value)
            .unwrap(),
        42
    );
    let singleton = vm
        .bind_function_application_declaration::<(i32,), (i32,)>(
            &_owner_program.root(),
            &singleton_id,
            &[i32_type],
        )
        .unwrap();
    let values = vm.call(&singleton, (42,)).unwrap();
    assert_eq!(values, (42,));

    let identity = vm
        .bind_function_application_declaration::<(Callback,), Callback>(
            &_owner_program.root(),
            &identity_id,
            &[callback_type],
        )
        .unwrap();
    let closure = vm
        .bind_function::<(), Callback>(&_owner_program.root(), "closure")
        .unwrap();
    let closure = vm.call(&closure, ()).unwrap();
    let callback = vm.call(&identity, (closure,)).unwrap();
    vm.collect_garbage().unwrap();
    assert_eq!(vm.call(&callback, ()).unwrap(), 42);
}

#[test]
fn generic_bindings_keep_type_evidence_without_pinning_ordinary_code() {
    let mut builder = ModuleBuilder::new(
        "example::generic_helper",
        &StandardDeclarations::default().catalog().unwrap(),
    )
    .unwrap();
    let identity = builder
        .define_function(FunctionDecl::new("identity"))
        .unwrap();
    builder
        .function(&identity, |function| {
            let value = function.type_parameter("T")?.ty();
            function.parameter("value", value.clone())?;
            function.returns(value)?;
            Ok(())
        })
        .unwrap();
    builder
        .bind(
            identity,
            NativeBinding::new(vec![Codec::Value], Codec::Value, |cx| cx.argument(0)),
        )
        .unwrap();
    let module = builder.finish().unwrap();
    let declaration = declaration_query(&module, DefinitionKind::Function, "identity");
    let source =
        include_str!("../../../../../tests/fixtures/native/generic-binding-type-evidence.kgr");
    let (mut vm, old) = fixture_without_foundation(source, Some(&module));
    let _old_program = old;
    let old = vm.select_program(&_old_program).unwrap();
    let item = vm.bind_type(&_old_program.root(), "Item", &[]).unwrap();
    let argument = item.type_argument().clone();
    let identity = vm
        .bind_function_application_declaration::<(Object,), Object>(
            &_old_program.root(),
            &declaration,
            slice::from_ref(&argument),
        )
        .unwrap();
    let duplicate = vm
        .bind_function_application_declaration::<(Object,), Object>(
            &_old_program.root(),
            &declaration,
            slice::from_ref(&argument),
        )
        .unwrap();
    let make = vm
        .bind_function::<(), Object>(&_old_program.root(), "make")
        .unwrap();
    let object = vm.call(&make, ()).unwrap();
    for pattern in ["pub val value: i32", "value: 41"] {
        assert_eq!(source.matches(pattern).count(), 1, "{pattern}");
    }
    let new_source = source
        .replace(
            "pub val value: i32",
            "pub val value: i32, pub val extra: bool",
        )
        .replace("value: 41", "value: 42, extra: true");
    assert!(
        reload_bytecode(
            &mut vm,
            &_old_program,
            Some("functions".into()),
            compile_without_foundation(&new_source, Some(&module))
        )
        .is_err()
    );
    // A rejected schema publication leaves the retained generic application usable.
    assert!(vm.call(&identity, (object.clone(),)).is_ok());
    let new_source = source.replace("value: 41", "value: 42");
    reload_bytecode(
        &mut vm,
        &_old_program,
        Some("functions".into()),
        compile_without_foundation(&new_source, Some(&module)),
    )
    .unwrap();
    let _new_program = _old_program.clone();
    let new = vm.select_program(&_new_program).unwrap();
    let current = vm
        .bind_function::<(), Object>(&_new_program.root(), "make")
        .unwrap();
    let current_object = vm.call(&current, ()).unwrap();
    let current_type = current_object.object_type();
    let current_value = vm.bind_field::<i32>(current_type, "value").unwrap();
    assert_eq!(
        current_object
            .get(&mut vm.context(&new).unwrap(), &current_value)
            .unwrap(),
        42
    );
    let retained = identity
        .call(&mut vm.context(&new).unwrap(), (object,))
        .unwrap();
    let value = vm.bind_field::<i32>(&item, "value").unwrap();
    assert_eq!(
        retained
            .get(&mut vm.context(&new).unwrap(), &value)
            .unwrap(),
        41
    );
    vm.collect_garbage().unwrap();
    assert!(
        vm.validate_executable_module(&old).is_ok(),
        "the returned object still owns its original type scope"
    );
    drop((retained, value, item, make, identity));
    vm.collect_garbage().unwrap();
    assert!(
        vm.validate_executable_module(&old).is_err(),
        "a reusable ordinary binding does not pin obsolete executable code"
    );
    assert!(vm.call(&duplicate, (current_object,)).is_ok());
    drop(duplicate);
}

#[test]
fn bound_native_application_uses_the_compilers_selected_trait_operation() {
    let mut builder = ModuleBuilder::new(
        "example::bound",
        &StandardDeclarations::default().catalog().unwrap(),
    )
    .unwrap();
    let mut step = builder.define_trait("Step").unwrap();
    step.define_method(
        MethodDecl::instance("step")
            .parameter("value", Type::i32())
            .returns(Type::i32()),
    )
    .unwrap();
    let step = step.finish().unwrap();
    let repeat = builder
        .define_function(FunctionDecl::new("repeat").returns(Type::i32()))
        .unwrap();
    let declaration = repeat.definitions().resolve(repeat.id()).unwrap().to_path();
    let selected = builder
        .function(&repeat, |function| {
            let item = function.type_parameter("T")?.ty();
            function.parameter("item", item.clone())?;
            function.bound(item.clone(), step.apply([])?)?;
            function.requires(CallableRequirement::method(item, step.method("step")?)?)
        })
        .unwrap();
    let effects = Arc::new(AtomicUsize::new(0));
    let observed = effects.clone();
    builder
        .bind(
            repeat,
            NativeBinding::new(vec![Codec::Value], Codec::Value, move |cx| {
                observed.fetch_add(1, Ordering::Relaxed);
                let item = cx.argument(0)?;
                let operation = cx.selected(&selected)?;
                cx.collect_garbage()?;
                cx.call_values(operation, &[item, ValueView::from(0i32)])
            }),
        )
        .unwrap();
    let module = builder.finish().unwrap();
    let (mut vm, owner) = fixture(
        include_str!("../../../../../tests/fixtures/native/function-bound-trait-operation.kgr"),
        Some(&module),
    );
    let _owner_program = owner;
    let owner = vm.select_program(&_owner_program).unwrap();
    let counter = vm
        .bind_type(&_owner_program.root(), "Counter", &[])
        .unwrap();
    let repeat = vm
        .bind_function_application_declaration::<(Object,), i32>(
            &_owner_program.root(),
            &declaration,
            slice::from_ref(counter.type_argument()),
        )
        .unwrap();
    let integer = ConversionContext::new(&mut vm, &owner)
        .unwrap()
        .type_for::<i32>()
        .unwrap();
    assert!(
        vm.bind_function_application_declaration::<(i32,), i32>(
            &_owner_program.root(),
            &declaration,
            &[integer]
        )
        .is_err()
    );
    assert_eq!(effects.load(Ordering::Relaxed), 0);
    let make = vm
        .bind_function::<(), Object>(&_owner_program.root(), "make")
        .unwrap();
    let input = vm.call(&make, ()).unwrap();
    assert_eq!(vm.call(&repeat, (input,)).unwrap(), 42);
    assert_eq!(effects.load(Ordering::Relaxed), 1);
}

#[test]
fn admitted_native_retains_its_frame_while_selected_callbacks_use_current_code() {
    let mut builder = ModuleBuilder::new(
        "example::admitted",
        &StandardDeclarations::default().catalog().unwrap(),
    )
    .unwrap();
    let mut step = builder.define_trait("Step").unwrap();
    step.define_method(MethodDecl::instance("step").returns(Type::i32()))
        .unwrap();
    let step = step.finish().unwrap();
    let invoke = builder
        .define_function(FunctionDecl::new("invoke").returns(Type::i32()))
        .unwrap();
    let declaration = invoke.definitions().resolve(invoke.id()).unwrap().to_path();
    let selected = builder
        .function(&invoke, |function| {
            let item = function.type_parameter("T")?.ty();
            function.parameter("item", item.clone())?;
            function.bound(item.clone(), step.apply([])?)?;
            function.requires(CallableRequirement::method(item, step.method("step")?)?)
        })
        .unwrap();
    let owners = Arc::new(Mutex::new(Vec::new()));
    let observed = owners.clone();
    builder
        .bind(
            invoke,
            NativeBinding::new(vec![Codec::Value], Codec::Value, move |cx| {
                let owner = cx.owner().key();
                observed.lock().unwrap().push(owner);
                let item = cx.argument(0)?;
                let operation = cx.selected(&selected)?;
                let continuation = 100;
                cx.collect_garbage()?;
                let result = cx
                    .call_values(operation, &[item])?
                    .as_i32()
                    .expect("checked callback result");
                cx.collect_garbage()?;
                assert_eq!(cx.owner().key(), owner);
                Ok(ValueView::from(continuation + result))
            }),
        )
        .unwrap();
    let module = builder.finish().unwrap();
    let source =
        include_str!("../../../../../tests/fixtures/native/admitted-selected-current-callback.kgr");
    let (mut vm, program) = fixture_without_foundation(source, Some(&module));
    let old = vm.select_program(&program).unwrap();
    vm.collect_garbage().unwrap();
    let baseline = vm.gc_stats();
    let counter = vm.bind_type(&program.root(), "Counter", &[]).unwrap();
    let value = vm.bind_field::<i32>(&counter, "value").unwrap();
    let receiver: Object = vm
        .execute_typed(&program, "make", (), &Default::default())
        .unwrap();
    let function = vm
        .bind_function_application_declaration::<(Object,), i32>(
            &program.root(),
            &declaration,
            slice::from_ref(counter.type_argument()),
        )
        .unwrap();
    let prepared = function.prepare_call(&mut vm).unwrap();
    let native_owner = prepared.owner().key();
    assert_eq!(
        prepared.owner().publication_token(),
        old.publication_token()
    );
    assert!(owners.lock().unwrap().is_empty());
    assert_eq!(source.matches("self.value += 1").count(), 1);
    let updated = source.replace("self.value += 1", "self.value += 10");
    let candidate = vm
        .stage_reload_program(
            &program,
            None,
            compile_without_foundation(&updated, Some(&module)),
        )
        .unwrap();
    {
        let mut _session = vm.begin_candidate_initialization(&candidate).unwrap();
        let options = _session.runtime_mut().execution_options();
        let error = prepared
            .call_with_options(
                &mut _session.runtime_mut().context(candidate.module()).unwrap(),
                (receiver.clone(),),
                options,
            )
            .unwrap_err();
        assert_eq!(error.kind(), RuntimeErrorKind::ExecutionPhaseViolation);
    }
    drop(candidate);
    let (mut foreign, foreign_program) = fixture_without_foundation(source, Some(&module));
    let foreign_owner = foreign.select_program(&foreign_program).unwrap();
    let options = foreign.execution_options();
    let error = prepared
        .call_with_options(
            &mut foreign.context(&foreign_owner).unwrap(),
            (receiver.clone(),),
            options,
        )
        .unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::StaleHandle);
    assert!(owners.lock().unwrap().is_empty());
    reload_bytecode(
        &mut vm,
        &program,
        None,
        compile_without_foundation(&updated, Some(&module)),
    )
    .unwrap();
    let current = vm.select_program(&program).unwrap();
    assert_ne!(
        current.publication_token(),
        prepared.owner().publication_token()
    );
    // Enter through the actual retained capability; a detached old descriptor
    // cannot reopen an obsolete publication through ordinary begin_execution.
    let mut activation = vm
        .enter_pinned_execution_stack(prepared.selection())
        .unwrap();
    activation
        .with(|vm, _| {
            let mut nested_activation = vm.enter_execution_stack(&current)?;
            nested_activation.with(|vm, _| {
                assert_eq!(vm.execution_root().unwrap().key(), native_owner);
                Ok(())
            })?;
            drop(nested_activation);
            for expected in [111, 121] {
                let options = vm.execution_options();
                assert_eq!(
                    prepared.call_with_options(
                        &mut vm.context(prepared.owner())?,
                        (receiver.clone(),),
                        options
                    )?,
                    expected
                );
                assert_eq!(vm.execution_root().unwrap().key(), native_owner);
                assert_eq!(vm.resources().counters().current_call_depth, 0);
            }
            Ok(())
        })
        .unwrap();
    drop(activation);
    assert_eq!(*owners.lock().unwrap(), [native_owner, native_owner]);
    assert_eq!(
        receiver
            .get(&mut vm.context(&current).unwrap(), &value)
            .unwrap(),
        21
    );
    assert!(vm.execution_root().is_none());
    drop((receiver, prepared, function, counter, value));
    vm.collect_garbage().unwrap();
    assert_eq!(vm.gc_stats().objects, baseline.objects);
    assert_eq!(
        vm.gc_stats().root_registrations,
        baseline.root_registrations
    );
}

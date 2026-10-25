use super::{compile_without_foundation, fixture, fixture_without_foundation};
use crate::tests::native_boundary::{
    native_boundary_interfaces::generic_identity_module, support::declaration_query,
};
use kagari_common::identity::{DefinitionKind, DefinitionPath, DefinitionPathSegment};
use kagari_runtime::{
    error::RuntimeErrorKind,
    native::{
        binding::{Codec, NativeBinding},
        builder::ModuleBuilder,
        collections::vector::ScriptVec,
        conversion::context::ConversionContext,
        declarations::{CallableRequirement, FunctionDecl, MethodDecl},
        function_handle::Function,
        objects::Object,
        types::Type,
    },
    value::Value,
};
use kagari_stdlib::declarations::StandardDeclarations;
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
    let (vm, owner) = fixture(
        r#"
        use example::generic_helper::identity;
        pub fn evidence() -> i32 { identity(42) }
        fn hidden<T>(value: T) -> T { value }
        pub fn hidden_evidence() -> i32 { hidden(1) }
    "#,
        Some(&module),
    );
    let _owner_program = owner;
    let owner = vm.runtime().select_program(&_owner_program).unwrap();
    let conversion = ConversionContext::new(vm.runtime(), &owner).unwrap();
    let i32_type = conversion.type_for::<i32>().unwrap();
    let string_type = conversion.type_for::<String>().unwrap();
    let identity = vm
        .runtime()
        .bind_function_application_declaration::<(i32,), i32>(
            &_owner_program.root(),
            &declaration,
            slice::from_ref(&i32_type),
        )
        .unwrap();
    assert_eq!(vm.call(&identity, (42,)).unwrap(), 42);
    assert!(
        vm.runtime()
            .bind_function::<(i32,), i32>(&_owner_program.root(), "identity")
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
        vm.runtime()
            .bind_function_application_declaration::<(String,), String>(
                &_owner_program.root(),
                &declaration,
                &[string_type]
            )
            .is_err()
    );
    assert!(
        vm.runtime()
            .bind_function_application_declaration::<(i32,), i32>(
                &_owner_program.root(),
                &hidden,
                slice::from_ref(&i32_type)
            )
            .is_err()
    );
    assert!(
        vm.runtime()
            .bind_function_application_declaration::<(i32,), i32>(
                &_owner_program.root(),
                &declaration,
                &[]
            )
            .is_err()
    );
    assert!(
        vm.runtime()
            .bind_function_application_declaration::<(i32,), bool>(
                &_owner_program.root(),
                &declaration,
                &[i32_type]
            )
            .is_err()
    );
}

#[test]
fn generic_native_calls_return_retained_objects_collections_and_closures() {
    let module = generic_identity_module();
    let identity_id = declaration_query(&module, DefinitionKind::Function, "identity");
    let singleton_id = declaration_query(&module, DefinitionKind::Function, "singleton");
    let (vm, owner) = fixture(
        r#"
        use example::generic_helper::{identity, singleton};
        pub struct Item { pub val value: i32 }
        pub fn make() -> Item { identity(Item { value: 42 }) }
        pub fn evidence() -> Vec<i32> { singleton(1) }
        pub fn closure() -> fn() -> i32 { identity(|| 42) }
    "#,
        Some(&module),
    );
    let _owner_program = owner;
    let owner = vm.runtime().select_program(&_owner_program).unwrap();
    let conversion = ConversionContext::new(vm.runtime(), &owner).unwrap();
    let i32_type = conversion.type_for::<i32>().unwrap();
    let item = vm
        .runtime()
        .bind_type(&_owner_program.root(), "Item", &[])
        .unwrap();
    let identity = vm
        .runtime()
        .bind_function_application_declaration::<(Object,), Object>(
            &_owner_program.root(),
            &identity_id,
            slice::from_ref(item.type_argument()),
        )
        .unwrap();
    let make = vm
        .runtime()
        .bind_function::<(), Object>(&_owner_program.root(), "make")
        .unwrap();
    let object = vm.call(&identity, (vm.call(&make, ()).unwrap(),)).unwrap();
    vm.runtime().collect_garbage().unwrap();
    let value = vm.runtime().bind_field::<i32>(&item, "value").unwrap();
    assert_eq!(
        object
            .get(&mut vm.context(&owner).unwrap(), &value)
            .unwrap(),
        42
    );
    let singleton = vm
        .runtime()
        .bind_function_application_declaration::<(i32,), ScriptVec<i32>>(
            &_owner_program.root(),
            &singleton_id,
            &[i32_type],
        )
        .unwrap();
    let values = vm.call(&singleton, (42,)).unwrap();
    assert_eq!(
        values.get(&mut vm.context(&owner).unwrap(), 0).unwrap(),
        Some(42)
    );
    type Callback = Function<(), i32>;
    let callback_type = conversion.type_for::<Callback>().unwrap();
    let identity = vm
        .runtime()
        .bind_function_application_declaration::<(Callback,), Callback>(
            &_owner_program.root(),
            &identity_id,
            &[callback_type],
        )
        .unwrap();
    let closure = vm
        .runtime()
        .bind_function::<(), Callback>(&_owner_program.root(), "closure")
        .unwrap();
    let callback = vm
        .call(&identity, (vm.call(&closure, ()).unwrap(),))
        .unwrap();
    vm.runtime().collect_garbage().unwrap();
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
        include_str!("../../../../tests/fixtures/native/generic-binding-type-evidence.kgr");
    let (vm, old) = fixture_without_foundation(source, Some(&module));
    let _old_program = old;
    let old = vm.runtime().select_program(&_old_program).unwrap();
    let item = vm
        .runtime()
        .bind_type(&_old_program.root(), "Item", &[])
        .unwrap();
    let argument = item.type_argument().clone();
    let identity = vm
        .runtime()
        .bind_function_application_declaration::<(Object,), Object>(
            &_old_program.root(),
            &declaration,
            slice::from_ref(&argument),
        )
        .unwrap();
    let duplicate = vm
        .runtime()
        .bind_function_application_declaration::<(Object,), Object>(
            &_old_program.root(),
            &declaration,
            slice::from_ref(&argument),
        )
        .unwrap();
    let make = vm
        .runtime()
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
        vm.reload_program(
            &_old_program,
            Some("functions".into()),
            compile_without_foundation(&new_source, Some(&module))
        )
        .is_err()
    );
    // A rejected schema publication leaves the retained generic application usable.
    assert!(vm.call(&identity, (object.clone(),)).is_ok());
    let new_source = source.replace("value: 41", "value: 42");
    vm.reload_program(
        &_old_program,
        Some("functions".into()),
        compile_without_foundation(&new_source, Some(&module)),
    )
    .unwrap();
    let _new_program = _old_program.clone();
    let new = vm.runtime().select_program(&_new_program).unwrap();
    let current = vm
        .runtime()
        .bind_function::<(), Object>(&_new_program.root(), "make")
        .unwrap();
    let current_object = vm.call(&current, ()).unwrap();
    let current_type = current_object.object_type();
    let current_value = vm
        .runtime()
        .bind_field::<i32>(current_type, "value")
        .unwrap();
    assert_eq!(
        current_object
            .get(&mut vm.context(&new).unwrap(), &current_value)
            .unwrap(),
        42
    );
    let retained = identity
        .call(&mut vm.context(&new).unwrap(), (object,))
        .unwrap();
    let value = vm.runtime().bind_field::<i32>(&item, "value").unwrap();
    assert_eq!(
        retained
            .get(&mut vm.context(&new).unwrap(), &value)
            .unwrap(),
        41
    );
    assert!(
        !vm.runtime()
            .collect_garbage()
            .unwrap()
            .reclaimed_modules
            .contains(&old.key()),
        "the returned object still owns its original type scope"
    );
    drop((retained, value, item, make, identity));
    assert!(
        vm.runtime()
            .collect_garbage()
            .unwrap()
            .reclaimed_modules
            .contains(&old.key()),
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
                cx.call_values(operation, &[item, Value::I32(0)])
            }),
        )
        .unwrap();
    let module = builder.finish().unwrap();
    let (vm, owner) = fixture(
        r#"
        use example::bound::{Step, repeat};
        pub struct Counter { pub val amount: i32 }
        impl Step for Counter { fn step(self, value: i32) -> i32 { value + self.amount } }
        pub fn make() -> Counter { Counter { amount: 42 } }
        pub fn evidence() -> i32 { repeat(make()) }
    "#,
        Some(&module),
    );
    let _owner_program = owner;
    let owner = vm.runtime().select_program(&_owner_program).unwrap();
    let counter = vm
        .runtime()
        .bind_type(&_owner_program.root(), "Counter", &[])
        .unwrap();
    let repeat = vm
        .runtime()
        .bind_function_application_declaration::<(Object,), i32>(
            &_owner_program.root(),
            &declaration,
            slice::from_ref(counter.type_argument()),
        )
        .unwrap();
    let integer = ConversionContext::new(vm.runtime(), &owner)
        .unwrap()
        .type_for::<i32>()
        .unwrap();
    assert!(
        vm.runtime()
            .bind_function_application_declaration::<(i32,), i32>(
                &_owner_program.root(),
                &declaration,
                &[integer]
            )
            .is_err()
    );
    assert_eq!(effects.load(Ordering::Relaxed), 0);
    let make = vm
        .runtime()
        .bind_function::<(), Object>(&_owner_program.root(), "make")
        .unwrap();
    assert_eq!(
        vm.call(&repeat, (vm.call(&make, ()).unwrap(),)).unwrap(),
        42
    );
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
                let Value::I32(result) = cx.call_values(operation, &[item])? else {
                    panic!("checked callback result");
                };
                cx.collect_garbage()?;
                assert_eq!(cx.owner().key(), owner);
                Ok(Value::I32(continuation + result))
            }),
        )
        .unwrap();
    let module = builder.finish().unwrap();
    let source =
        include_str!("../../../../tests/fixtures/native/admitted-selected-current-callback.kgr");
    let (vm, program) = fixture_without_foundation(source, Some(&module));
    let old = vm.runtime().select_program(&program).unwrap();
    let counter = vm
        .runtime()
        .bind_type(&program.root(), "Counter", &[])
        .unwrap();
    let value = vm.runtime().bind_field::<i32>(&counter, "value").unwrap();
    let receiver: Object = vm.execute_typed(&old, "make", ()).unwrap();
    let function = vm
        .runtime()
        .bind_function_application_declaration::<(Object,), i32>(
            &program.root(),
            &declaration,
            slice::from_ref(counter.type_argument()),
        )
        .unwrap();
    let prepared = function.prepare_call(vm.runtime()).unwrap();
    let native_owner = prepared.owner().key();
    assert_eq!(
        prepared.owner().publication_token(),
        old.publication_token()
    );
    assert!(owners.lock().unwrap().is_empty());
    assert_eq!(source.matches("self.value += 1").count(), 1);
    let updated = source.replace("self.value += 1", "self.value += 10");
    let candidate = vm
        .runtime()
        .stage_reload_program(
            &program,
            None,
            compile_without_foundation(&updated, Some(&module)),
        )
        .unwrap();
    {
        let _session = vm
            .runtime()
            .begin_candidate_initialization(&candidate)
            .unwrap();
        let error = prepared
            .call_with_options::<_, i32>(
                &mut vm.context(candidate.module()).unwrap(),
                (receiver.clone(),),
                vm.runtime().execution_options(),
            )
            .unwrap_err();
        assert_eq!(error.kind(), RuntimeErrorKind::ExecutionPhaseViolation);
    }
    drop(candidate);
    let (foreign, foreign_program) = fixture_without_foundation(source, Some(&module));
    let foreign_owner = foreign.runtime().select_program(&foreign_program).unwrap();
    let error = prepared
        .call_with_options::<_, i32>(
            &mut foreign.context(&foreign_owner).unwrap(),
            (receiver.clone(),),
            foreign.runtime().execution_options(),
        )
        .unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::StaleHandle);
    assert!(owners.lock().unwrap().is_empty());
    vm.reload_program(
        &program,
        None,
        compile_without_foundation(&updated, Some(&module)),
    )
    .unwrap();
    let current = vm.runtime().select_program(&program).unwrap();
    assert_ne!(
        current.publication_token(),
        prepared.owner().publication_token()
    );
    {
        let _session = vm
            .runtime()
            .begin_execution(prepared.owner(), vm.runtime().execution_options())
            .unwrap();
        // Current entries from the same installation may join the admitted session.
        let nested = vm.runtime().enter_execution_stack(&current).unwrap();
        assert_eq!(vm.runtime().execution_root().unwrap().key(), native_owner);
        drop(nested);
        for expected in [111, 121] {
            assert_eq!(
                prepared
                    .call_with_options::<_, i32>(
                        &mut vm.context(prepared.owner()).unwrap(),
                        (receiver.clone(),),
                        vm.runtime().execution_options(),
                    )
                    .unwrap(),
                expected
            );
            assert_eq!(vm.runtime().execution_root().unwrap().key(), native_owner);
            assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
        }
    }
    assert_eq!(*owners.lock().unwrap(), [native_owner, native_owner]);
    assert_eq!(
        receiver
            .get(&mut vm.context(&current).unwrap(), &value)
            .unwrap(),
        21
    );
    assert!(vm.runtime().execution_root().is_none());
    drop((receiver, prepared, function, counter, value));
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
    assert_eq!(vm.runtime().gc().active_roots(), 0);
}

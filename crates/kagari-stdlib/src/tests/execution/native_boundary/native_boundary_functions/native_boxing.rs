use super::fixture;
use crate::{
    declarations::StandardDeclarations,
    tests::{
        execution::native_boundary::{
            compile_program,
            native_boundary_interfaces::generic_identity_module,
            support::{declaration_query, round_trip_program},
        },
        support::bytecode::reload_bytecode,
    },
};
use kagari_runtime::{
    Runtime,
    error::{RuntimeError, RuntimeErrorKind},
    execution::owned::DriveResult,
    native::{
        binding::{Codec, NativeBinding, NativeResult},
        builder::ModuleBuilder,
        conversion::{KagariType, context::ConversionContext},
        declarations::FunctionDecl,
        function_handle::Function,
        module::NativeModule,
        objects::Object,
        typed::NativeContext,
        types::Type,
    },
    value::view::ValueView,
};

#[cfg(test)]
use crate::tests::support::bytecode::load_bytecode;

use kagari_common::{cancellation::CancellationToken, identity::DefinitionKind};

use std::{
    num::NonZeroUsize,
    slice,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

type Add = Function<(i32, i32), i32>;
type Callback = Function<(), i32>;

#[test]
fn registered_native_entries_box_round_trip_and_run_inside_script_closures() {
    let calls = Arc::new(AtomicUsize::new(0));
    let text_calls = Arc::new(AtomicUsize::new(0));
    let mut module = ModuleBuilder::new(
        "example::boxed",
        &StandardDeclarations::default().catalog().unwrap(),
    )
    .unwrap();
    let observed = calls.clone();
    let add = {
        let registered_function = module
            .define_function(
                FunctionDecl::new("add")
                    .parameter("left", Type::i32())
                    .parameter("right", Type::i32())
                    .returns(Type::i32()),
            )
            .unwrap();
        module
            .bind(
                registered_function.clone(),
                move |cx: &mut NativeContext<'_>, (left, right): (i32, i32)| -> NativeResult<i32> {
                    cx.collect_garbage()?;
                    observed.fetch_add(1, Ordering::Relaxed);
                    if left < 0 {
                        return Err(RuntimeError::module_validation("requested failure"));
                    }
                    Ok(left + right)
                },
            )
            .map(|()| registered_function)
    }
    .unwrap();
    let observed = text_calls.clone();
    let text = {
        let registered_function = module
            .define_function(
                FunctionDecl::new("text")
                    .returns(<String>::kagari_type(module.declarations()).unwrap()),
            )
            .unwrap();
        module
            .bind(
                registered_function.clone(),
                move |_cx: &mut NativeContext<'_>, (): ()| -> NativeResult<String> {
                    observed.fetch_add(1, Ordering::Relaxed);
                    Ok("retained native result".to_owned())
                },
            )
            .map(|()| registered_function)
    }
    .unwrap();
    let through = module
        .define_function(
            FunctionDecl::new("through")
                .parameter(
                    "f",
                    Type::function([Type::i32(), Type::i32()], Type::i32()).unwrap(),
                )
                .returns(Type::i32()),
        )
        .unwrap();
    module
        .bind(
            through,
            NativeBinding::new(vec![Codec::Value], Codec::Exact(Type::i32()), |cx| {
                let callback = cx.callable(0)?;
                callback.call_values(cx, &[ValueView::from(20i32), ValueView::from(22i32)])
            }),
        )
        .unwrap();
    let module = module.finish().unwrap();
    let source = include_str!("../../../../../tests/fixtures/native/boxed-native-callbacks.kgr");
    let (mut vm, old) = fixture(source, Some(&module));
    let _old_program = old;
    let old = vm.select_program(&_old_program).unwrap();
    vm.collect_garbage().unwrap();
    let baseline = vm.gc_stats();
    // Native completion can park before returning to its script caller. Its
    // frame must retain the result without repeating the callback on resume.
    let callback = vm
        .bind_function_declaration::<(), String>(
            &_old_program.root(),
            &text.definitions().resolve(text.id()).unwrap().to_path(),
        )
        .unwrap();
    let callback = ConversionContext::new(&mut vm, &old)
        .unwrap()
        .encode(callback)
        .unwrap();
    let owner = vm
        .start(
            &old,
            "owned",
            slice::from_ref(&callback),
            Default::default(),
        )
        .unwrap();
    drop(callback);
    let mut parked_native_return = false;
    let mut completed = false;
    for _ in 0..32 {
        vm.collect_garbage().unwrap();
        let before = text_calls.load(Ordering::Relaxed);
        match vm.drive(&owner, NonZeroUsize::new(1).unwrap()).unwrap() {
            DriveResult::Runnable => {
                parked_native_return |= before == 0 && text_calls.load(Ordering::Relaxed) == 1;
            }
            DriveResult::Waiting => panic!("synchronous native entry cannot wait"),
            DriveResult::Complete(result) => {
                let result = result.unwrap();
                assert_eq!(
                    ConversionContext::new(&mut vm, &old)
                        .unwrap()
                        .decode::<String>(&result)
                        .unwrap(),
                    "retained native result"
                );
                completed = true;
                break;
            }
        }
    }
    assert!(parked_native_return && completed);
    assert_eq!(text_calls.load(Ordering::Relaxed), 1);
    drop(owner);
    let function = vm
        .bind_function_declaration::<(i32, i32), i32>(
            &_old_program.root(),
            &add.definitions().resolve(add.id()).unwrap().to_path(),
        )
        .unwrap();
    assert_eq!(
        vm.execute_typed::<_, i32>(
            &_old_program,
            "invoke",
            (function.clone(), 20, 22),
            &Default::default()
        )
        .unwrap(),
        42
    );
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert!(
        vm.execute_typed::<_, i32>(
            &_old_program,
            "wrong",
            (function.clone(),),
            &Default::default()
        )
        .is_err()
    );
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    let returned: Add = vm
        .execute_typed(
            &_old_program,
            "echo",
            (function.clone(),),
            &Default::default(),
        )
        .unwrap();
    assert_eq!(
        vm.execute_typed::<_, i32>(
            &_old_program,
            "native",
            (function.clone(),),
            &Default::default()
        )
        .unwrap(),
        42
    );
    assert_eq!(vm.call(&returned, (19, 23)).unwrap(), 42);
    let callback: Callback = vm
        .execute_typed(
            &_old_program,
            "capture",
            (function.clone(),),
            &Default::default(),
        )
        .unwrap();
    drop(function);
    reload_bytecode(
        &mut vm,
        &_old_program,
        Some("functions".into()),
        compile_program(source, Some(&module)),
    )
    .unwrap();
    let _current_program = _old_program.clone();
    let current = vm.select_program(&_current_program).unwrap();
    assert_eq!(vm.call(&callback, ()).unwrap(), 42);
    assert_eq!(
        vm.execute_typed::<_, i32>(
            &_current_program,
            "invoke",
            (returned.clone(), 21, 21),
            &Default::default()
        )
        .unwrap(),
        42
    );
    assert!(
        vm.execute_typed::<_, i32>(
            &_current_program,
            "invoke",
            (returned.clone(), -1, 0),
            &Default::default()
        )
        .is_err()
    );
    assert_eq!(vm.resources().counters().current_call_depth, 0);
    assert!(vm.execution_root().is_none());
    let before = calls.load(Ordering::Relaxed);
    let cancellation = CancellationToken::default();
    let mut options = vm.execution_options();
    options.cancellation = cancellation.clone();
    {
        let mut _session = vm.begin_execution(&current, options).unwrap();
        cancellation.cancel();
        assert!(_session.runtime_mut().call(&returned, (20, 22)).is_err());
    }
    assert_eq!(calls.load(Ordering::Relaxed), before);
    assert_eq!(vm.call(&returned, (20, 22)).unwrap(), 42);
    let (mut foreign, _) = fixture(source, Some(&module));

    assert!(foreign.call(&returned, (20, 22)).is_err());
    drop((returned, callback));
    vm.collect_garbage().unwrap();
    assert!(vm.validate_executable_module(&old).is_err());
    assert_eq!(vm.gc_stats().objects, baseline.objects);
    assert_eq!(
        vm.gc_stats().root_registrations,
        baseline.root_registrations
    );
}

#[test]
fn generic_native_function_values_retain_applied_nominal_scopes() {
    type Identity = Function<(Object,), Object>;
    let module = generic_identity_module();
    let declaration = declaration_query(&module, DefinitionKind::Function, "identity");
    let source = include_str!("../../../../../tests/fixtures/native/boxed-native-nominal.kgr");
    let (mut vm, old) = fixture(source, Some(&module));
    let _old_program = old;
    let old = vm.select_program(&_old_program).unwrap();
    vm.collect_garbage().unwrap();
    let baseline = vm.gc_stats();
    let item_type = vm.bind_type(&_old_program.root(), "Item", &[]).unwrap();
    let identity = vm
        .bind_function_application_declaration::<(Object,), Object>(
            &_old_program.root(),
            &declaration,
            slice::from_ref(item_type.type_argument()),
        )
        .unwrap();
    let item: Object = vm
        .execute_typed(&_old_program, "item", (), &Default::default())
        .unwrap();
    let echoed: Identity = vm
        .execute_typed(&_old_program, "echo", (identity,), &Default::default())
        .unwrap();
    let returned: Object = vm
        .execute_typed(
            &_old_program,
            "invoke",
            (echoed.clone(), item.clone()),
            &Default::default(),
        )
        .unwrap();
    let value = vm.bind_field::<i32>(&item_type, "value").unwrap();
    returned
        .set(&mut vm.context(&old).unwrap(), &value, 43)
        .unwrap();
    assert_eq!(
        item.get(&mut vm.context(&old).unwrap(), &value).unwrap(),
        43
    );
    vm.collect_garbage().unwrap();
    let before_independent = vm.gc_stats();
    let new = load_bytecode(
        &mut vm,
        "independent",
        compile_program(&source.replace("i32", "i64"), Some(&module)),
    )
    .unwrap();
    let _new_program = new;
    let _new = vm.select_program(&_new_program).unwrap();
    vm.collect_garbage().unwrap();
    let publication_objects = vm.gc_stats().objects - before_independent.objects;
    let publication_roots =
        vm.gc_stats().root_registrations - before_independent.root_registrations;
    let other: Object = vm
        .execute_typed(&_new_program, "item", (), &Default::default())
        .unwrap();
    assert!(
        vm.execute_typed::<_, Object>(
            &_new_program,
            "invoke",
            (echoed.clone(), other.clone()),
            &Default::default()
        )
        .is_err()
    );
    assert!(vm.call(&echoed, (other,)).is_err());
    assert!(vm.call(&echoed, (item.clone(),)).is_ok());
    drop((echoed, item, returned, value, item_type));
    vm.collect_garbage().unwrap();
    assert_eq!(
        vm.gc_stats().objects,
        baseline.objects + publication_objects
    );
    assert_eq!(
        vm.gc_stats().root_registrations,
        baseline.root_registrations + publication_roots
    );
}

#[test]
fn native_function_values_cannot_invent_a_missing_nested_application() {
    let module = generic_identity_module();
    let declaration = declaration_query(&module, DefinitionKind::Function, "identity");
    let (mut vm, owner) = fixture(
        include_str!("../../../../../tests/fixtures/native/boxed-native-nested-evidence.kgr"),
        Some(&module),
    );
    let _owner_program = owner;
    let owner = vm.select_program(&_owner_program).unwrap();
    let ty = ConversionContext::new(&mut vm, &owner)
        .unwrap()
        .type_for::<i32>()
        .unwrap();
    let function = vm.bind_function_application_declaration::<(i32,), i32>(
        &_owner_program.root(),
        &declaration,
        &[ty],
    );
    // No closed witness exists for the nested native application in this open
    // relay body; boxing must not invent evidence to bind an unavailable entry.
    assert_eq!(
        function.unwrap_err().kind(),
        RuntimeErrorKind::ModuleValidation
    );
}

#[test]
fn a_boxed_named_native_entry_does_not_retain_its_obsolete_program() {
    let mut module = ModuleBuilder::new(
        "example::boxed",
        &StandardDeclarations::default().catalog().unwrap(),
    )
    .unwrap();
    let add = {
        let registered_function = module
            .define_function(
                FunctionDecl::new("add")
                    .parameter("a", Type::i32())
                    .parameter("b", Type::i32())
                    .returns(Type::i32()),
            )
            .unwrap();
        module
            .bind(
                registered_function.clone(),
                |_: &mut NativeContext<'_>, (a, b): (i32, i32)| -> NativeResult<i32> { Ok(a + b) },
            )
            .map(|()| registered_function)
    }
    .unwrap();
    let module = module.finish().unwrap();
    let program = compile_program(
        include_str!("../../../../../tests/fixtures/native/boxed-native-entry.kgr"),
        Some(&module),
    );
    let program = round_trip_program(program);
    let mut runtime = Runtime::default();
    NativeModule::install_all(&crate::modules().unwrap(), &mut runtime).unwrap();
    module.install(&mut runtime).unwrap();
    let old = load_bytecode(&mut runtime, "cycle", program.clone()).unwrap();
    let _old_program = old;
    let old = runtime.select_program(&_old_program).unwrap();
    let mut vm = runtime;
    vm.collect_garbage().unwrap();
    let baseline = vm.gc_stats();
    let native = vm
        .bind_function_declaration::<(i32, i32), i32>(
            &_old_program.root(),
            &add.definitions().resolve(add.id()).unwrap().to_path(),
        )
        .unwrap();
    let root = ConversionContext::new(&mut vm, &old)
        .unwrap()
        .encode(native)
        .unwrap();
    {
        let mut cx = ConversionContext::new(&mut vm, &old).unwrap();
        let value = cx.import(&root).unwrap();
        assert!(
            cx.runtime_mut()
                .resolve_closure(&value)
                .unwrap()
                .script_function()
                .is_none()
        );
    }
    reload_bytecode(&mut vm, &_old_program, Some("cycle".into()), program).unwrap();
    vm.collect_garbage().unwrap();
    assert!(vm.validate_executable_module(&old).is_err());
    let current = vm.select_program(&_old_program).unwrap();
    let callback: Add = ConversionContext::new(&mut vm, &current)
        .unwrap()
        .decode(&root)
        .unwrap();
    assert_eq!(vm.call(&callback, (20, 22)).unwrap(), 42);
    drop((callback, root));
    vm.collect_garbage().unwrap();
    assert!(vm.validate_executable_module(&current).is_ok());
    assert_eq!(vm.gc_stats().objects, baseline.objects);
    assert_eq!(
        vm.gc_stats().root_registrations,
        baseline.root_registrations
    );
}

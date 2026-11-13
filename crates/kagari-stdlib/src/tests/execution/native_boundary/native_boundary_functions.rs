#[cfg(test)]
use crate::tests::support::bytecode::load_bytecode;
use crate::{
    declarations::StandardDeclarations,
    tests::{
        execution::native_boundary::support::round_trip_program, support::bytecode::reload_bytecode,
    },
};
mod applications;
mod interface_applications;
mod interface_handles;
mod interface_methods;
mod method_applications;
mod methods;
mod native_boxing;
mod primitives;
mod resource_termination;
mod script_values;
mod selected_methods;
mod shared_application;
use super::{compile_program, compile_without_foundation};
use kagari_runtime::{
    Runtime, RuntimeConfig,
    error::RuntimeErrorKind,
    execution::memory::GcHeapConfig,
    frame::types::arguments::TypeArgument,
    module::InstalledProgram,
    native::{
        binding::NativeResult,
        builder::ModuleBuilder,
        catalog::DeclarationCatalog,
        conversion::{
            FromKagari, IntoKagari, KagariType, arguments::PreparedArguments,
            context::ConversionContext,
        },
        declarations::FunctionDecl,
        function_handle::Function,
        module::NativeModule,
        objects::Object,
        typed::NativeContext,
        types::Type,
    },
    resource::RuntimeLimits,
    value::view::ValueView,
};

use kagari_bytecode::program::BytecodeProgram;
use kagari_common::cancellation::CancellationToken;
use kagari_common::identity::metadata::DefinitionMetadata;
use kagari_types::{scalar::BuiltinType, ty::Ty};

use std::{
    cell::Cell,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

type ScalarCallback = Function<(i32,), i32>;

pub(super) fn fixture(source: &str, module: Option<&NativeModule>) -> (Runtime, InstalledProgram) {
    let mut modules = crate::modules().unwrap();
    modules.extend(module.cloned());
    fixture_program(compile_program(source, module), modules.iter())
}

pub(super) fn fixture_without_foundation(
    source: &str,
    module: Option<&NativeModule>,
) -> (Runtime, InstalledProgram) {
    fixture_program(compile_without_foundation(source, module), module)
}

fn fixture_program<'a>(
    program: DefinitionMetadata<BytecodeProgram>,
    modules: impl IntoIterator<Item = &'a NativeModule>,
) -> (Runtime, InstalledProgram) {
    let program = round_trip_program(program);
    let mut runtime = Runtime::new(RuntimeConfig {
        gc: GcHeapConfig {
            collection_threshold: Some(1),
            ..Default::default()
        },
        limits: RuntimeLimits {
            max_call_depth: Some(16),
        },
        ..Default::default()
    });
    let modules = modules.into_iter().cloned().collect::<Vec<_>>();
    NativeModule::install_all(&modules, &mut runtime).unwrap();
    let owner = load_bytecode(&mut runtime, "functions", program).unwrap();
    (runtime, owner)
}

#[test]
fn bound_entries_check_visibility_signature_and_return_retained_objects() {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut module = ModuleBuilder::new(
        "example::functions",
        &StandardDeclarations::default().catalog().unwrap(),
    )
    .unwrap();
    let observed = calls.clone();
    {
        let registered_function = module.define_function(FunctionDecl::new("tick")).unwrap();
        module
            .bind(
                registered_function.clone(),
                move |_: &mut NativeContext<'_>, (): ()| -> NativeResult<()> {
                    observed.fetch_add(1, Ordering::Relaxed);
                    Ok(())
                },
            )
            .map(|()| registered_function)
    }
    .unwrap();
    let (mut vm, owner) = fixture(
        include_str!("../../../../tests/fixtures/native/function-bound-object-entries.kgr"),
        Some(&module.finish().unwrap()),
    );
    let _owner_program = owner;
    vm.collect_garbage().unwrap();
    let baseline = vm.gc_stats();
    assert!(
        vm.bind_function::<(), i32>(&_owner_program.root(), "private")
            .is_err()
    );
    assert!(
        vm.bind_function::<(), i32>(&_owner_program.root(), "make")
            .is_err()
    );
    assert!(
        vm.bind_function::<(i32,), Object>(&_owner_program.root(), "make")
            .is_err()
    );
    assert_eq!(calls.load(Ordering::Relaxed), 0);
    let make = vm
        .bind_function::<(), Object>(&_owner_program.root(), "make")
        .unwrap();
    let read = vm
        .bind_function::<(Object,), i32>(&_owner_program.root(), "read")
        .unwrap();
    let player = vm.call(&make, ()).unwrap();
    vm.collect_garbage().unwrap();
    assert_eq!(vm.call(&read, (player.clone(),)).unwrap(), 42);
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    let (mut foreign, _) = fixture("pub fn main() {}", None);

    assert!(foreign.call(&make, ()).is_err());
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    drop(player);
    vm.collect_garbage().unwrap();
    assert_eq!(vm.gc_stats().objects, baseline.objects);
    assert_eq!(
        vm.gc_stats().root_registrations,
        baseline.root_registrations
    );
}

#[test]
fn typed_closures_reenter_native_and_retain_captures_until_last_handle() {
    let catalog = StandardDeclarations::default().catalog().unwrap();
    let remembered: Arc<Mutex<Option<ScalarCallback>>> = Arc::default();
    let mut module = ModuleBuilder::new("example::functions", &catalog).unwrap();
    let saved = remembered.clone();
    {
        let registered_function = module
            .define_function(
                FunctionDecl::new("apply")
                    .parameter(
                        "callback",
                        <Function<(i32,), i32>>::kagari_type(module.declarations()).unwrap(),
                    )
                    .parameter("value", Type::i32())
                    .returns(Type::i32()),
            )
            .unwrap();
        module
            .bind(
                registered_function.clone(),
                move |cx: &mut NativeContext<'_>,
                      (callback, value): (Function<(i32,), i32>, i32)|
                      -> NativeResult<i32> {
                    *saved.lock().unwrap() = Some(callback.clone());
                    cx.collect_garbage()?;
                    callback.call(cx, (value,))
                },
            )
            .map(|()| registered_function)
    }
    .unwrap();
    {
        let registered_function = module
            .define_function(
                FunctionDecl::new("increment")
                    .parameter("value", Type::i32())
                    .returns(Type::i32()),
            )
            .unwrap();
        module
            .bind(
                registered_function.clone(),
                |cx: &mut NativeContext<'_>, (value,): (i32,)| -> NativeResult<i32> {
                    cx.collect_garbage()?;
                    Ok(value + 1)
                },
            )
            .map(|()| registered_function)
    }
    .unwrap();
    let (mut vm, owner) = fixture(
        include_str!("../../../../tests/fixtures/native/function-captured-reentry.kgr"),
        Some(&module.finish().unwrap()),
    );
    let _owner_program = owner;
    vm.collect_garbage().unwrap();
    let baseline = vm.gc_stats();
    let main = vm
        .bind_function::<(), i32>(&_owner_program.root(), "main")
        .unwrap();
    assert_eq!(vm.call(&main, ()).unwrap(), 42);
    let callback = remembered.lock().unwrap().take().unwrap();
    vm.collect_garbage().unwrap();
    assert_eq!(vm.call(&callback, (2,)).unwrap(), 43);
    let invoke = vm
        .bind_function::<(Function<(i32,), i32>,), i32>(&_owner_program.root(), "use_callback")
        .unwrap();
    assert_eq!(vm.call(&invoke, (callback.clone(),)).unwrap(), 43);
    drop(callback);
    vm.collect_garbage().unwrap();
    assert_eq!(vm.gc_stats().objects, baseline.objects);
    assert_eq!(
        vm.gc_stats().root_registrations,
        baseline.root_registrations
    );
}

#[test]
fn returned_closures_retain_bodies_while_bound_functions_select_current() {
    let source = |answer| {
        format!(
            r#"
            mod implementation {{
                pub fn value() -> i32 {{ {answer} }}
                pub fn closure() -> fn() -> i32 {{ val data = [{answer}]; || data[0] }}
            }}
            pub use crate::implementation::value as answer;
            pub use crate::implementation::closure as make;
            pub use crate as again;
            fn main() {{}}
            "#
        )
    };
    let (mut vm, old) = fixture_without_foundation(&source(41), None);
    let _old_program = old;
    let old = vm.select_program(&_old_program).unwrap();
    vm.collect_garbage().unwrap();
    let baseline = vm.gc_stats();
    let answer = vm
        .bind_function::<(), i32>(&_old_program.root(), "again::answer")
        .unwrap();
    assert_eq!(
        answer
            .prepare_call(&mut vm)
            .unwrap()
            .owner()
            .bytecode
            .identity
            .path,
        ["implementation"]
    );
    for inaccessible in [
        "main",
        "value",
        "implementation::value",
        "::answer",
        "again",
    ] {
        assert!(
            vm.bind_function::<(), i32>(&_old_program.root(), inaccessible)
                .is_err(),
            "{inaccessible}"
        );
    }
    let duplicate = vm
        .bind_function::<(), i32>(&_old_program.root(), "answer")
        .unwrap();
    let make = vm
        .bind_function::<(), Function<(), i32>>(&_old_program.root(), "make")
        .unwrap();
    let closure = vm.call(&make, ()).unwrap();
    let prepared = answer.prepare_call(&mut vm).unwrap();
    reload_bytecode(
        &mut vm,
        &_old_program,
        Some("functions".into()),
        compile_without_foundation(&source(42), None),
    )
    .unwrap();
    let _new_program = _old_program.clone();
    let new = vm.select_program(&_new_program).unwrap();
    let new_answer = vm
        .bind_function::<(), i32>(&_new_program.root(), "answer")
        .unwrap();
    {
        let mut session = vm.begin_execution(&new, vm.execution_options()).unwrap();
        let options = session.runtime().execution_options();
        let mut cx = session.runtime_mut().context(&new).unwrap();
        assert_eq!(answer.call(&mut cx, ()).unwrap(), 42);
        assert_eq!(
            prepared.call_with_options(&mut cx, (), options).unwrap(),
            41
        );
        assert_eq!(closure.call(&mut cx, ()).unwrap(), 41);
        assert_eq!(new_answer.call(&mut cx, ()).unwrap(), 42);
    }
    drop(prepared);
    drop(answer);
    drop(make);
    vm.collect_garbage().unwrap();
    assert!(
        vm.validate_executable_module(&old).is_ok(),
        "the retained closure keeps its exact body alive"
    );
    drop(closure);
    vm.collect_garbage().unwrap();
    assert!(
        vm.validate_executable_module(&old).is_err(),
        "ordinary handles must not retain obsolete executable bodies"
    );
    assert_eq!(vm.call(&duplicate, ()).unwrap(), 42);
    drop(duplicate);
    vm.collect_garbage().unwrap();
    assert_eq!(vm.gc_stats().objects, baseline.objects);
    assert_eq!(
        vm.gc_stats().root_registrations,
        baseline.root_registrations
    );
}

#[test]
fn public_script_function_handles_can_be_passed_back_as_callable_values() {
    let (mut vm, owner) = fixture(
        include_str!("../../../../tests/fixtures/native/function-callable-entry.kgr"),
        None,
    );
    let _owner_program = owner;
    vm.collect_garbage().unwrap();
    let baseline = vm.gc_stats();
    let double = vm
        .bind_function::<(i32,), i32>(&_owner_program.root(), "double")
        .unwrap();
    let invoke = vm
        .bind_function::<(Function<(i32,), i32>,), i32>(&_owner_program.root(), "use_callback")
        .unwrap();
    assert_eq!(vm.call(&invoke, (double,)).unwrap(), 42);
    vm.collect_garbage().unwrap();
    assert_eq!(vm.gc_stats().objects, baseline.objects);
    assert_eq!(
        vm.gc_stats().root_registrations,
        baseline.root_registrations
    );
}

#[test]
fn typed_reentry_traps_cancellation_and_depth_exhaustion_release_execution_state() {
    let mut module = ModuleBuilder::new(
        "example::functions",
        &StandardDeclarations::default().catalog().unwrap(),
    )
    .unwrap();
    {
        let registered_function = module
            .define_function(
                FunctionDecl::new("apply")
                    .parameter(
                        "callback",
                        <Function<(i32,), i32>>::kagari_type(module.declarations()).unwrap(),
                    )
                    .parameter("value", Type::i32())
                    .returns(Type::i32()),
            )
            .unwrap();
        module
            .bind(
                registered_function.clone(),
                |cx: &mut NativeContext<'_>,
                 (callback, value): (Function<(i32,), i32>, i32)|
                 -> NativeResult<i32> { callback.call(cx, (value,)) },
            )
            .map(|()| registered_function)
    }
    .unwrap();
    let (mut vm, owner) = fixture(
        include_str!("../../../../tests/fixtures/native/function-reentry-failures.kgr"),
        Some(&module.finish().unwrap()),
    );
    let _owner_program = owner;
    let owner = vm.select_program(&_owner_program).unwrap();
    vm.collect_garbage().unwrap();
    let baseline = vm.gc_stats();
    let divide = vm
        .bind_function::<(i32,), i32>(&_owner_program.root(), "divide")
        .unwrap();
    let recurse = vm
        .bind_function::<(i32,), i32>(&_owner_program.root(), "recurse")
        .unwrap();
    let healthy = vm
        .bind_function::<(), i32>(&_owner_program.root(), "healthy")
        .unwrap();
    assert!(vm.call(&divide, (0,)).is_err());
    assert!(
        matches!(vm.call(&recurse, (1,)), Err(error) if error.kind() == RuntimeErrorKind::ResourceLimitExceeded)
    );
    assert_eq!(vm.resources().counters().current_call_depth, 0);
    assert!(vm.execution_root().is_none());
    vm.collect_garbage().unwrap();
    assert_eq!(vm.gc_stats().objects, baseline.objects);
    assert_eq!(
        vm.gc_stats().root_registrations,
        baseline.root_registrations
    );
    let token = CancellationToken::default();
    let mut options = vm.execution_options();
    options.cancellation = token.clone();
    {
        let mut _session = vm.begin_execution(&owner, options).unwrap();
        token.cancel();
        assert!(
            matches!(_session.runtime_mut().call(&healthy, ()), Err(error) if error.kind() == RuntimeErrorKind::Cancelled)
        );
    }
    assert_eq!(vm.call(&healthy, ()).unwrap(), 42);
    assert_eq!(vm.resources().counters().current_call_depth, 0);
}

#[test]
fn retained_entry_cannot_escape_candidate_initialization_isolation() {
    let source = "pub fn answer() -> i32 { 42 }";
    let (mut vm, old) = fixture(source, None);
    let _old_program = old;
    let old = vm.select_program(&_old_program).unwrap();
    let old_function = vm
        .bind_function::<(), i32>(&_old_program.root(), "answer")
        .unwrap();
    let candidate = vm
        .stage_reload_program(
            &_old_program,
            Some("functions".into()),
            compile_program(source, None),
        )
        .unwrap();
    let new_function = vm
        .bind_function::<(), i32>(&_old_program.root(), "answer")
        .unwrap();
    let prepared = new_function.prepare_call(&mut vm).unwrap();
    assert_eq!(prepared.owner().key(), old.key());
    assert_ne!(prepared.owner().key(), candidate.module().key());
    {
        let mut _session = vm.begin_candidate_initialization(&candidate).unwrap();
        assert!(
            matches!(_session.runtime_mut().call(&old_function, ()), Err(error) if error.kind() == RuntimeErrorKind::ExecutionPhaseViolation)
        );
        assert!(
            matches!(_session.runtime_mut().call(&new_function, ()), Err(error) if error.kind() == RuntimeErrorKind::ExecutionPhaseViolation)
        );
    }
    assert_eq!(vm.call(&old_function, ()).unwrap(), 42);
}

thread_local! {
    static SCALAR_EFFECTS: Cell<(usize, usize, usize)> = const { Cell::new((0, 0, 0)) };
}

#[derive(Debug, PartialEq)]
struct MappedScalar(i32);

impl KagariType for MappedScalar {
    fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
        let (mapping, encode, decode) = SCALAR_EFFECTS.get();
        SCALAR_EFFECTS.set((mapping + 1, encode, decode));
        i32::kagari_type(catalog)
    }
}

impl IntoKagari for MappedScalar {
    fn into_kagari<'call>(
        self,
        cx: &mut ConversionContext<'call>,
        expected: &TypeArgument,
    ) -> NativeResult<ValueView<'call>> {
        let (mapping, encode, decode) = SCALAR_EFFECTS.get();
        SCALAR_EFFECTS.set((mapping, encode + 1, decode));
        self.0.into_kagari(cx, expected)
    }
}

impl FromKagari for MappedScalar {
    fn from_kagari(
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
        value: &ValueView<'_>,
    ) -> NativeResult<Self> {
        let (mapping, encode, decode) = SCALAR_EFFECTS.get();
        SCALAR_EFFECTS.set((mapping, encode, decode + 1));
        i32::from_kagari(cx, expected, value).map(Self)
    }
}

#[test]
fn native_declaration_handles_bind_checked_installed_entries_directly() {
    let mut module = ModuleBuilder::new(
        "example::functions",
        &StandardDeclarations::default().catalog().unwrap(),
    )
    .unwrap();
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
                |_: &mut NativeContext<'_>, (left, right): (i32, i32)| -> NativeResult<i32> {
                    Ok(left + right)
                },
            )
            .map(|()| registered_function)
    }
    .unwrap();
    let (mut vm, owner) = fixture(
        include_str!("../../../../tests/fixtures/native/function-native-declaration.kgr"),
        Some(&module.finish().unwrap()),
    );
    let _owner_program = owner;
    SCALAR_EFFECTS.set((0, 0, 0));
    let declaration = add.definitions().resolve(add.id()).unwrap().to_path();
    let function = vm
        .bind_function_declaration::<(MappedScalar, MappedScalar), MappedScalar>(
            &_owner_program.root(),
            &declaration,
        )
        .unwrap();
    assert_eq!(SCALAR_EFFECTS.get(), (3, 0, 0));
    let duplicate = vm
        .bind_function_declaration::<(MappedScalar, MappedScalar), MappedScalar>(
            &_owner_program.root(),
            &declaration,
        )
        .unwrap();
    for function in [&function, &duplicate] {
        assert_eq!(
            vm.call(function, (MappedScalar(20), MappedScalar(22)))
                .unwrap(),
            MappedScalar(42)
        );
    }
    let prepared = function.prepare_call(&mut vm).unwrap().clone();
    let options = vm.execution_options();
    let mut cx = vm.context(prepared.owner()).unwrap();
    assert_eq!(
        prepared
            .call_with_options(&mut cx, (MappedScalar(19), MappedScalar(23)), options)
            .unwrap(),
        MappedScalar(42)
    );
    assert_eq!(SCALAR_EFFECTS.get(), (3, 6, 3));
    drop(cx);

    // Full-pack admission precedes even the first custom decoder. The valid
    // first scalar must produce no effect when a later argument has wrong type.
    let types = vm
        .resolve_type_arguments(
            prepared.owner(),
            &[Ty::Builtin(BuiltinType::I32), Ty::Builtin(BuiltinType::I32)],
        )
        .unwrap();
    let mut cx = ConversionContext::new(&mut vm, prepared.owner()).unwrap();
    let mapping = PreparedArguments::<(MappedScalar, MappedScalar)>::new(&cx, &types).unwrap();
    let before = SCALAR_EFFECTS.get();
    let values = cx
        .argument_values(&[ValueView::from(20i32), ValueView::from(true)])
        .unwrap();
    assert_eq!(
        mapping.decode(&mut cx, values).unwrap_err().kind(),
        RuntimeErrorKind::ModuleValidation
    );
    assert_eq!(SCALAR_EFFECTS.get(), before);
    let roots = cx.runtime().gc_stats().root_registrations;
    let values = mapping
        .encode(&mut cx, (MappedScalar(19), MappedScalar(23)))
        .unwrap();
    assert_eq!(
        mapping.decode(&mut cx, values).unwrap(),
        (MappedScalar(19), MappedScalar(23))
    );
    assert_eq!(SCALAR_EFFECTS.get(), (before.0, before.1 + 2, before.2 + 2));
    assert_eq!(cx.runtime().gc_stats().root_registrations, roots);
}

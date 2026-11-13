#[cfg(test)]
use crate::test_support::bytecode::load_bytecode;
use crate::test_support::native::program::ProgramFixture;
use crate::{
    Runtime,
    error::{RuntimeErrorKind, reason::ErrorReason, trap::TrapReason},
    module::ExecutableModule,
    native::conversion::context::ConversionContext,
    value::{carriers::RootedValue, strings::StringObject},
};
use kagari_bytecode::{module::BytecodeModule, program::ModuleRef};
use kagari_common::identity::map::DefinitionContext;
use kagari_gc::error::GcError;
use kagari_types::{scalar::BuiltinType, ty::Ty};
use std::panic::{AssertUnwindSafe, catch_unwind};

fn runtime_with_owner() -> (Runtime, ExecutableModule) {
    let context = DefinitionContext::new().unwrap();
    let mut runtime = Runtime::default();
    let owner_program = load_bytecode(
        &mut runtime,
        "allocations",
        ProgramFixture {
            exports: Default::default(),
            root: ModuleRef::new(0),
            modules: vec![BytecodeModule::default()],
        }
        .metadata(context.snapshot(), &Default::default())
        .unwrap(),
    )
    .unwrap();
    let owner = runtime.select_program(&owner_program).unwrap();
    (runtime, owner)
}

#[test]
fn rejected_allocations_preserve_managed_state_counters_and_existing_values() {
    let (mut runtime, owner) = runtime_with_owner();
    let first = runtime
        .alloc_array(
            &owner,
            Ty::Builtin(BuiltinType::I32),
            &[RootedValue::from(1i32)],
        )
        .unwrap();
    let before = runtime.resources().counters();
    let heap_before = runtime.gc_stats();
    let error = runtime
        .alloc_array(
            &owner,
            Ty::Builtin(BuiltinType::I32),
            &[RootedValue::from(true)],
        )
        .unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::ScriptTrap);
    assert!(matches!(
        error.reason(),
        ErrorReason::Trap(TrapReason::InvalidSequenceBufferOrPayload)
    ));
    assert_eq!(runtime.resources().counters(), before);
    assert_eq!(runtime.gc_stats(), heap_before);
    let mut source = ConversionContext::new(&mut runtime, &owner).unwrap();
    let array = source.import(&first).unwrap();
    assert_eq!(source.array_len(&array).unwrap(), 1);
    assert_eq!(
        source
            .array_element(&array, 0usize)
            .unwrap()
            .unwrap()
            .as_i32(),
        Some(1)
    );
    drop(source);
    assert!(!runtime.is_quarantined());
}

#[test]
fn committed_allocations_are_attributed_when_the_physical_scope_unwinds() {
    let (mut runtime, owner) = runtime_with_owner();
    runtime.collect_garbage().unwrap();
    let baseline = runtime.gc_stats();
    let baseline_units = baseline.objects as u128 + baseline.provider_units;
    let options = runtime.execution_options();
    let mut session = runtime.begin_execution(&owner, options).unwrap();
    const TEXT: &str = "committed before unwind";
    assert!(
        catch_unwind(AssertUnwindSafe(|| {
            let Runtime {
                memory, resources, ..
            } = session.runtime_mut();
            let _: Result<(), GcError> = memory.scope(resources, |cx, _| {
                StringObject::alloc(cx, TEXT.to_owned())?;
                panic!("physical scope unwind after commit");
            });
        }))
        .is_err()
    );
    let expected = baseline_units + TEXT.len() as u128 + 1;
    assert_eq!(session.counters().current_heap_units, expected);
    assert_eq!(session.counters().peak_heap_units, expected);
    assert_eq!(session.runtime().gc_stats().objects, baseline.objects + 1);
    let resources = session.runtime().resources().counters();
    assert_eq!(
        resources.current_retained_bytes,
        session.runtime().gc_stats().indirect_bytes as u128
    );
    assert!(
        resources.current_retained_bytes >= baseline.indirect_bytes as u128 + TEXT.len() as u128
    );
    session.runtime_mut().collect_garbage().unwrap();
    assert_eq!(session.counters().current_heap_units, baseline_units);
    assert_eq!(session.counters().peak_heap_units, expected);
    assert_eq!(session.runtime().gc_stats().objects, baseline.objects);
    assert_eq!(
        session.runtime().gc_stats().root_registrations,
        baseline.root_registrations
    );
    assert_eq!(
        session
            .runtime()
            .resources()
            .counters()
            .current_retained_bytes,
        baseline.indirect_bytes as u128
    );
    assert!(!session.runtime().is_quarantined());
}

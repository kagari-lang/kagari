use super::load;
use crate::{
    Runtime,
    error::RuntimeErrorKind,
    native::{
        binding::NativeResult,
        catalog::DeclarationCatalog,
        conversion::{KagariType, context::ConversionContext},
        types::Type,
    },
    test_support::native::program::ProgramFixture,
};
use kagari_bytecode::{module::BytecodeModule, program::ModuleRef};
use kagari_common::identity::map::DefinitionContext;
use std::panic::{AssertUnwindSafe, catch_unwind};

#[test]
fn exact_execution_retention_covers_members_without_another_registration_or_store_borrow() {
    fn assert_send<T: Send>() {}
    assert_send::<Runtime>();
    let mut runtime = Runtime::default();
    let mut modules: Vec<_> = (0..40)
        .map(|index| {
            let mut module = BytecodeModule::default();
            module.identity.path.push(format!("member{index}"));
            module
        })
        .collect();
    modules[2].dependencies = (0..40)
        .filter(|index| *index != 2)
        .map(ModuleRef::new)
        .collect();
    let code = ProgramFixture {
        exports: Default::default(),
        root: ModuleRef::new(2),
        modules,
    }
    .metadata(
        DefinitionContext::new().unwrap().snapshot(),
        &Default::default(),
    )
    .unwrap();
    let program = runtime.load_program("conversion-members", code).unwrap();
    let root = runtime.select_program(&program).unwrap();
    let member = root.member(ModuleRef::new(39)).unwrap();
    let options = runtime.execution_options();
    let cancellation = options.cancellation.clone();
    let session = runtime.begin_execution(&root, options).unwrap();
    let authority = session.retention();
    assert_eq!(
        runtime.modules().retention_counts(root.key()).active_calls,
        1
    );
    {
        let _standalone = ConversionContext::new(&runtime, &member).unwrap();
        assert_eq!(
            runtime.modules().retention_counts(root.key()).active_calls,
            2
        );
    }
    let mut conversion = ConversionContext::in_execution(&runtime, &member, &authority).unwrap();
    assert_eq!(
        runtime.modules().retention_counts(root.key()).active_calls,
        1
    );
    let value = conversion.encode("retained result".to_owned()).unwrap();
    {
        let nested = runtime
            .begin_execution(&member, runtime.execution_options())
            .unwrap();
        runtime.collect_garbage().unwrap();
        assert_eq!(
            conversion.decode::<String>(&value).unwrap(),
            "retained result"
        );
        drop(nested);
    }
    cancellation.cancel();
    let roots = runtime.gc().active_roots();
    assert_eq!(
        conversion
            .encode("cancelled".to_owned())
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::Cancelled
    );
    assert_eq!(runtime.gc().active_roots(), roots);
    drop(conversion);
    drop(session);
    assert!(runtime.resources().termination().is_none());
    assert_eq!(
        runtime.modules().retention_counts(root.key()).active_calls,
        0
    );
    runtime.collect_garbage().unwrap();
    assert!(value.value(runtime.gc()).is_some());
}

#[test]
fn execution_retention_is_exact_not_an_ambient_session_permission() {
    let mut runtime = Runtime::default();
    let program = load(&mut runtime, "retained");
    let other = load(&mut runtime, "other");
    let root = runtime.select_program(&program).unwrap();
    let unrelated = runtime.select_program(&other).unwrap();
    let candidate = runtime
        .stage_reload_verified_program(&program, None, root.verified_program().clone())
        .unwrap();
    let replacement = candidate.module().clone();
    let session = runtime
        .begin_execution(&root, runtime.execution_options())
        .unwrap();
    let authority = session.retention();
    for owner in [&unrelated, &replacement] {
        assert!(!authority.covers(&runtime, owner).unwrap());
        let before = runtime.modules().retention_counts(owner.key()).active_calls;
        let conversion = ConversionContext::in_execution(&runtime, owner, &authority).unwrap();
        assert_eq!(
            runtime.modules().retention_counts(owner.key()).active_calls,
            before + 1
        );
        drop(conversion);
        assert_eq!(
            runtime.modules().retention_counts(owner.key()).active_calls,
            before
        );
    }
    drop(candidate);
    assert!(ConversionContext::in_execution(&runtime, &replacement, &authority).is_err());
    let mut foreign = Runtime::default();
    let foreign_program = load(&mut foreign, "foreign");
    let foreign_owner = foreign.select_program(&foreign_program).unwrap();
    assert!(ConversionContext::in_execution(&runtime, &foreign_owner, &authority).is_err());
    assert!(ConversionContext::in_execution(&foreign, &foreign_owner, &authority).is_err());
}

#[test]
fn borrowed_conversion_retains_dynamic_admission_and_poison_checks() {
    enum Fault {
        Quarantine,
        Poison,
        Explicit,
    }

    struct UnreachableMapping;

    impl KagariType for UnreachableMapping {
        fn kagari_type(_: &DeclarationCatalog) -> NativeResult<Type> {
            panic!("graph admission must precede custom type mapping and its errors");
        }
    }

    for fault in [Fault::Quarantine, Fault::Poison, Fault::Explicit] {
        let mut runtime = Runtime::default();
        let program = load(&mut runtime, "dynamic-checks");
        let owner = runtime.select_program(&program).unwrap();
        let session = runtime
            .begin_execution(&owner, runtime.execution_options())
            .unwrap();
        let authority = session.retention();
        match fault {
            Fault::Quarantine => {
                runtime.resources().quarantine("conversion admission fault");
            }
            Fault::Poison => {
                assert!(
                    catch_unwind(AssertUnwindSafe(|| {
                        let _graph = runtime.gc.graph.lock();
                        panic!("injected graph poison");
                    }))
                    .is_err()
                );
            }
            Fault::Explicit => {
                runtime.gc.graph.try_lock().unwrap().fault();
                assert!(!runtime.is_quarantined());
                // Diagnostic availability is allowed after an explicit fault;
                // it is not authority to publish a conversion's program owner.
                runtime.validate_executable_module(&owner).unwrap();
                assert!(authority.covers(&runtime, &owner).unwrap());
            }
        }
        assert!(ConversionContext::new(&runtime, &owner).is_err());
        assert!(
            ConversionContext::in_execution(&runtime, &owner, &authority)
                .and_then(|cx| cx.type_for::<UnreachableMapping>())
                .is_err()
        );
    }
}

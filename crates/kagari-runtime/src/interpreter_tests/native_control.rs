//! Cooperative Native polling and post-call cancellation use one root session.
use crate::{
    Runtime,
    error::RuntimeErrorKind,
    interpreter_tests::common::compile_native_test_bytecode,
    module::entry::ProgramEntry,
    native::{
        binding::NativeResult, builder::ModuleBuilder, context::NativeCall,
        declarations::FunctionDecl, types::Type,
    },
    test_support::bytecode::load_bytecode,
};
use kagari_common::cancellation::CancellationToken;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[test]
fn native_poll_observes_cancellation_and_return_cannot_swallow_it() {
    for swallow in [false, true] {
        let token = CancellationToken::default();
        let cancel = token.clone();
        let visited = Arc::new(AtomicUsize::new(0));
        let observed = visited.clone();
        let mut native = ModuleBuilder::new("test::polling", &Default::default()).unwrap();
        let work = native
            .define_function(FunctionDecl::new("work").returns(Type::i32()))
            .unwrap();
        native
            .bind(work, move |cx: &mut NativeCall<'_>| -> NativeResult<i32> {
                for index in 0..8 {
                    if index == 3 {
                        cancel.cancel();
                    }
                    if let Err(error) = cx.poll() {
                        return if swallow { Ok(42) } else { Err(error) };
                    }
                    observed.fetch_add(1, Ordering::SeqCst);
                }
                Ok(42)
            })
            .unwrap();
        let native = native.finish().unwrap();
        let program = compile_native_test_bytecode(
            include_str!("../../tests/fixtures/native/native-control-poll.kgr"),
            &[&native],
        );
        let mut runtime = Runtime::default();
        native.install(&mut runtime).unwrap();
        let loaded = load_bytecode(&mut runtime, "native-poll", program).unwrap();
        let _loaded_program = loaded;
        let loaded = runtime.select_program(&_loaded_program).unwrap();
        runtime.collect_garbage().unwrap();
        let baseline = runtime.gc_stats();
        let mut options = runtime.execution_options();
        options.cancellation = token;
        let mut vm = runtime;
        let mut session = vm.begin_execution(&loaded, options).unwrap();
        let session_options = session.runtime().execution_options();

        let error = session
            .runtime_mut()
            .execute_module(&loaded, ProgramEntry::Main, session_options.clone())
            .unwrap_err();
        assert!(matches!(&error, error if error.kind() == RuntimeErrorKind::Cancelled));
        assert_eq!(visited.load(Ordering::SeqCst), 3);

        assert_eq!(
            session.runtime().resources().counters().current_call_depth,
            0
        );
        drop(session);
        assert!(vm.resources().poll_execution().is_ok());
        vm.collect_garbage().unwrap();
        assert_eq!(
            vm.gc_stats().root_registrations,
            baseline.root_registrations
        );
        assert_eq!(vm.gc_stats().objects, baseline.objects);
    }
}

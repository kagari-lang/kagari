//! Deterministic fake IO. Only the host driver performs script conversion/execution.
use kagari_runtime::native::{
    binding::NativeResult, builder::ModuleBuilder, catalog::DeclarationCatalog,
    completion::Completion, future::NativeStart, module::NativeModule, registration::FunctionSpec,
};
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

pub struct Request {
    pub service: &'static str,
    pub input: i32,
    pub completion: Completion<i32>,
}

#[derive(Clone, Default)]
pub struct FakeIo {
    requests: Arc<Mutex<VecDeque<Request>>>,
    started: Arc<AtomicUsize>,
    cancelled: Arc<AtomicUsize>,
}

impl FakeIo {
    pub fn module(
        &self,
        service: &'static str,
        declarations: &DeclarationCatalog,
    ) -> NativeResult<NativeModule> {
        let mut module = ModuleBuilder::new(service, declarations);
        let io = self.clone();
        module.add_async_function::<(i32,), i32>(
            FunctionSpec::new("request").parameter_names(["input"]),
            move |(input,), completion| {
                io.started.fetch_add(1, Ordering::SeqCst);
                io.requests.lock().unwrap().push_back(Request {
                    service,
                    input,
                    completion,
                });
                let cancelled = io.cancelled.clone();
                Ok(NativeStart::cancellable(move || {
                    cancelled.fetch_add(1, Ordering::SeqCst);
                }))
            },
        )?;
        module.finish()
    }

    pub fn take_request(&self) -> Option<Request> {
        self.requests.lock().unwrap().pop_front()
    }

    pub fn started(&self) -> usize {
        self.started.load(Ordering::SeqCst)
    }

    pub fn cancelled(&self) -> usize {
        self.cancelled.load(Ordering::SeqCst)
    }
}

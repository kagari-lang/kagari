//! Request input waits and fair worker admission; feature handlers own operation flow.
mod input;

#[cfg(test)]
use crate::session::analysis::testing::CompletionGate;

use crate::{
    analysis::{AnalysisFailure, AnalysisState, features::ProtocolOutput},
    protocol::{
        ErrorCode,
        output::request_lifecycle_reserve_bytes,
        typed::{TypedRequest, check_encoded_size},
    },
    session::{Lifecycle, SessionActor},
    workspace::{ContextKey, WorkspaceCapture},
};
use kagari_common::cancellation::CancellationToken;
#[cfg(all(test, feature = "verification"))]
use kagari_hir::analysis::work::milestone::{self, BodyCheckGate};
use lattice_actor::{
    context::ActorContext,
    traits::{Handler, Message},
};
use lsp_types::request::Request as LspRequest;
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    io::{self, Error},
    sync::Arc,
};
#[cfg(all(test, feature = "verification"))]
use tokio::sync::watch;
use tokio::sync::{oneshot, oneshot::error::RecvError};

type Dispatch = Box<
    dyn FnOnce(
            &mut SessionActor,
            &mut ActorContext<SessionActor>,
            Arc<WorkspaceCapture>,
        ) -> io::Result<()>
        + Send,
>;

struct RequestTask {
    serial: u64,
    generation: u64,
    source_name: String,
    capture: Option<Arc<WorkspaceCapture>>,
    dispatch: Dispatch,
}

#[derive(Default)]
pub(crate) struct RequestTasks {
    pending: VecDeque<RequestTask>,
    active: Option<Arc<WorkspaceCapture>>,
    served: BTreeMap<Option<ContextKey>, u64>,
    turn: u64,
    #[cfg(test)]
    pub step_gate: Option<CompletionGate>,
    #[cfg(all(test, feature = "verification"))]
    pub gate: Option<BodyCheckGate>,
    #[cfg(all(test, feature = "verification"))]
    pub started: watch::Sender<(u64, u64)>,
}

impl RequestTasks {
    fn next_ready(&mut self) -> io::Result<Option<RequestTask>> {
        let Some((index, key)) = self
            .pending
            .iter()
            .enumerate()
            .filter_map(|(index, task)| {
                let capture = task.capture.as_ref()?;
                let key = capture
                    .document_contexts
                    .get(&task.source_name)
                    .map(|selected| selected.context.clone());
                Some((index, key))
            })
            .min_by_key(|(_, key)| self.served.get(key).copied().unwrap_or(0))
        else {
            return Ok(None);
        };
        self.turn = self
            .turn
            .checked_add(1)
            .ok_or_else(|| Error::other("request scheduler turn exhausted"))?;
        self.served.insert(key.clone(), self.turn);
        let task = self.pending.remove(index).expect("selected request task");
        let mut live_keys: BTreeSet<_> = self
            .pending
            .iter()
            .filter_map(|task| {
                let capture = task.capture.as_ref()?;
                Some(
                    capture
                        .document_contexts
                        .get(&task.source_name)
                        .map(|selected| selected.context.clone()),
                )
            })
            .collect();
        // Keep this turn's owner while its worker may return a continuation.
        live_keys.insert(key);
        self.served.retain(|key, _| live_keys.contains(key));
        Ok(Some(task))
    }

    pub fn waiting_generations(&self) -> BTreeSet<u64> {
        self.pending
            .iter()
            .filter(|task| task.capture.is_none())
            .map(|task| task.generation)
            .collect()
    }

    pub fn waiting_for(&self, generation: u64) -> bool {
        self.pending
            .iter()
            .any(|task| task.generation == generation && task.capture.is_none())
    }

    pub fn accept_capture(&mut self, capture: &Arc<WorkspaceCapture>) {
        for task in &mut self.pending {
            if task.generation == capture.generation {
                task.capture = Some(capture.clone());
            }
        }
    }

    pub fn retained_contexts(&self) -> BTreeSet<ContextKey> {
        self.pending
            .iter()
            .filter_map(|task| task.capture.as_ref())
            .chain(self.active.iter())
            .flat_map(|capture| &capture.contexts)
            .map(|context| context.key.clone())
            .collect()
    }

    pub fn cancel(&mut self, serial: u64) {
        self.pending.retain(|task| task.serial != serial);
    }

    pub fn shutdown(&mut self) {
        self.pending.clear();
        self.served.clear();
        // The active worker retains its own capture until its matching completion.
    }
}

/// Completion metadata is resource-only. Each feature supplies its own Lattice message.
pub(crate) struct WorkCompletion<R> {
    pub job: u64,
    pub serial: u64,
    pub capture: Arc<WorkspaceCapture>,
    pub result: Result<R, RecvError>,
}

pub(crate) struct Admitted<R: LspRequest> {
    pub serial: u64,
    pub params: R::Params,
}

impl SessionActor {
    pub fn admit_read<R: LspRequest>(
        &mut self,
        ctx: &mut ActorContext<Self>,
        request: TypedRequest<R>,
    ) -> io::Result<Option<Admitted<R>>> {
        let Some(serial) = self.admit(ctx, request.id, R::METHOD) else {
            return Ok(None);
        };
        if !self.require_running(serial)? {
            return Ok(None);
        }
        let params = match request.params {
            Ok(params) => params,
            Err(_) => {
                self.reject(
                    serial,
                    ErrorCode::InvalidParams,
                    "invalid method parameters",
                )?;
                return Ok(None);
            }
        };
        if check_encoded_size(&params, 64 * 1024).is_err() {
            self.reject(
                serial,
                ErrorCode::InvalidParams,
                "semantic parameters exceed 64 KiB",
            )?;
            return Ok(None);
        }
        Ok(Some(Admitted { serial, params }))
    }

    pub fn enqueue_request(
        &mut self,
        serial: u64,
        generation: u64,
        source_name: String,
        capture: Option<Arc<WorkspaceCapture>>,
        dispatch: Dispatch,
    ) {
        self.operations.pending.push_back(RequestTask {
            serial,
            generation,
            source_name,
            capture,
            dispatch,
        });
        if self.analysis.preemptible {
            self.analysis.credit.cancel();
        }
    }

    pub fn cancel_request_work(&mut self, serial: u64) {
        self.operations.cancel(serial);
        self.analysis.maintenance = true;
        self.captures
            .cancel_unneeded(&self.operations.waiting_generations());
    }

    pub fn fail_input_waiters(&mut self, generation: u64, message: &str) -> io::Result<()> {
        let failed: Vec<_> = self
            .operations
            .pending
            .iter()
            .filter(|task| task.generation == generation)
            .map(|task| task.serial)
            .collect();
        for serial in failed {
            self.cancel_request_work(serial);
            self.reject(serial, ErrorCode::RequestFailed, message)?;
        }
        Ok(())
    }

    pub fn schedule_request_work(&mut self, ctx: &mut ActorContext<Self>) -> io::Result<bool> {
        let Some(mut task) = self.operations.next_ready()? else {
            return Ok(false);
        };
        let capture = task.capture.take().expect("ready request task");
        (task.dispatch)(self, ctx, capture)?;
        Ok(true)
    }

    pub fn submit_request_work<R, M, F>(
        &mut self,
        ctx: &mut ActorContext<Self>,
        serial: u64,
        capture: Arc<WorkspaceCapture>,
        execute: F,
        complete: impl FnOnce(WorkCompletion<R>) -> M + Send + 'static,
    ) -> io::Result<()>
    where
        R: Send + 'static,
        M: Message,
        Self: Handler<M>,
        F: FnOnce(&mut AnalysisState, &CancellationToken, u64) -> R + Send + 'static,
    {
        let token = self
            .requests
            .token(serial)
            .ok_or_else(|| Error::other("work lost request ownership"))?;
        let job = self.analysis.credit.reserve(token.clone())?;
        let (sender, receiver) = oneshot::channel();
        let completion_capture = capture.clone();
        ctx.pipe_to_self(receiver, move |result| {
            complete(WorkCompletion {
                job,
                serial,
                capture: completion_capture,
                result,
            })
        })
        .map_err(Error::other)?;
        self.operations.active = Some(capture.clone());
        let mut retained = self.operations.retained_contexts();
        retained.extend(self.analysis.live_contexts.iter().cloned());
        let live_serials = self.requests.unresolved().into_iter().collect();
        let latest_capture = self.captures.latest.clone();
        #[cfg(all(test, feature = "verification"))]
        let gate = self.operations.gate.take();
        #[cfg(all(test, feature = "verification"))]
        self.operations
            .started
            .send_replace((job, capture.generation));
        self.analysis.maintenance = false;
        self.analysis
            .port
            .as_ref()
            .ok_or_else(|| Error::other("analysis worker unavailable"))?
            .submit(
                move |state, cancel| {
                    #[cfg(all(test, feature = "verification"))]
                    let _milestone = gate
                        .map(|gate| milestone::install(gate).expect("one worker milestone scope"));
                    state.retain_contexts(&retained);
                    state.retain_requests(&live_serials);
                    if let Some(capture) = &latest_capture {
                        state.retain_capture_inputs(capture);
                    }
                    execute(state, cancel, job)
                },
                token,
                sender,
            )
            .map_err(Error::other)?;
        Ok(())
    }

    pub fn accept_request_work<R>(
        &mut self,
        completion: WorkCompletion<R>,
    ) -> io::Result<Option<R>> {
        self.analysis.credit.complete(completion.job)?;
        #[cfg(test)]
        self.analysis
            .completed
            .send_replace((completion.job, completion.capture.generation));
        self.operations.active = None;
        if completion
            .capture
            .contexts
            .iter()
            .any(|context| !self.analysis.live_contexts.contains(&context.key))
        {
            self.analysis.maintenance = true;
        }
        let result = completion
            .result
            .map_err(|_| Error::other("analysis worker lost request completion"))?;
        let Some(token) = self.requests.token(completion.serial) else {
            return Ok(None);
        };
        if self.state != Lifecycle::Running || token.check().is_err() {
            return Ok(None);
        }
        Ok(Some(result))
    }

    pub fn respond_request(
        &mut self,
        serial: u64,
        result: Result<ProtocolOutput, AnalysisFailure>,
    ) -> io::Result<()> {
        let output = match result {
            Ok(output) => output,
            Err(AnalysisFailure::Cancelled) => {
                return self.reject(serial, ErrorCode::RequestCancelled, "query cancelled");
            }
            Err(error) => return self.reject(serial, ErrorCode::RequestFailed, &error.to_string()),
        };
        let reserve = request_lifecycle_reserve_bytes(self.requests.unresolved().len());
        if !self.outbox.can_push_payload(output.charge, reserve) {
            return self.reject(
                serial,
                ErrorCode::RequestFailed,
                "semantic result exceeds available output capacity",
            );
        }
        self.finish_charged(serial, Ok(output.value), output.charge)
    }
}

#[cfg(test)]
mod tests;

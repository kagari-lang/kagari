//! Typed feature admission retains the exact input barrier until its terminal decision.
mod progress;

#[cfg(test)]
use crate::session::analysis::testing::CompletionGate;

use crate::{
    analysis::{
        AnalysisFailure,
        features::{FeatureInput, FeatureOutput, FeatureQuery, QueryStep},
    },
    protocol::{ErrorCode, Request, source_name},
    session::{Lifecycle, SessionActor, feature_scheduler::progress::ProgressStreams},
    workspace::{ContextKey, WorkspaceCapture},
};
#[cfg(all(test, feature = "verification"))]
use kagari_hir::analysis::work::milestone::{self, BodyCheckGate};
use kagari_source::line_index::{Position, PositionEncoding};
use lattice_actor::{
    context::{ActorContext, HandlerContext},
    traits::Handler,
};
use lsp_types::{ProgressToken, TextDocumentPositionParams};
use serde_json::from_value;
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    io::{self, Error},
    marker::PhantomData,
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

struct FeatureTask {
    serial: u64,
    generation: u64,
    source_name: String,
    capture: Option<Arc<WorkspaceCapture>>,
    dispatch: Dispatch,
}

#[derive(Default)]
pub(crate) struct FeatureScheduler {
    pending: VecDeque<FeatureTask>,
    active: Option<Arc<WorkspaceCapture>>,
    served: BTreeMap<Option<ContextKey>, u64>,
    turn: u64,
    pub progress: ProgressStreams,
    #[cfg(test)]
    pub step_gate: Option<CompletionGate>,
    #[cfg(all(test, feature = "verification"))]
    pub gate: Option<BodyCheckGate>,
    #[cfg(all(test, feature = "verification"))]
    pub started: watch::Sender<(u64, u64)>,
}

impl FeatureScheduler {
    fn next_ready(&mut self) -> io::Result<Option<FeatureTask>> {
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
            .ok_or_else(|| Error::other("feature scheduler turn exhausted"))?;
        self.served.insert(key.clone(), self.turn);
        let task = self.pending.remove(index).expect("selected feature task");
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

#[derive(lattice_actor::Message)]
struct FeatureFinished<Q: FeatureQuery> {
    job: u64,
    input: FeatureInput,
    result: Result<Result<QueryStep, AnalysisFailure>, RecvError>,
    query: PhantomData<Q>,
}

impl SessionActor {
    pub fn admit_feature<Q: FeatureQuery>(
        &mut self,
        ctx: &mut ActorContext<Self>,
        request: Request,
    ) -> io::Result<()> {
        let Some(serial) = self.admit(ctx, request.id, Q::METHOD) else {
            return Ok(());
        };
        if !self.require_running(serial)? {
            return Ok(());
        }
        let progress = if Q::WORK_DONE && self.features.progress.supported {
            match request.params.get("workDoneToken") {
                Some(value) => match from_value::<ProgressToken>(value.clone()) {
                    Ok(ProgressToken::String(token)) if token.len() <= 256 => {
                        Some(ProgressToken::String(token))
                    }
                    Ok(ProgressToken::Number(token)) => Some(ProgressToken::Number(token)),
                    _ => {
                        return self.reject(
                            serial,
                            ErrorCode::InvalidParams,
                            "invalid or oversized work-done token",
                        );
                    }
                },
                None => None,
            }
        } else {
            None
        };
        let params = match from_value::<TextDocumentPositionParams>(request.params) {
            Ok(params) => params,
            Err(_) => {
                return self.reject(
                    serial,
                    ErrorCode::InvalidParams,
                    "invalid document position parameters",
                );
            }
        };
        let name = match source_name(&params.text_document.uri) {
            Ok(name) => name,
            Err(error) => return self.reject(serial, ErrorCode::InvalidParams, &error),
        };
        let documents: Arc<[_]> = self.documents.inputs().into();
        let Some(document) = documents
            .iter()
            .find(|document| document.source_name == name)
        else {
            return self.reject(
                serial,
                ErrorCode::InvalidParams,
                "query document is not open",
            );
        };
        let Some(source) = &document.source else {
            return self.reject(
                serial,
                ErrorCode::RequestFailed,
                "query document text is unavailable",
            );
        };
        let Some(offset) = source.offset(
            Position {
                line: params.position.line as usize,
                character: params.position.character as usize,
            },
            PositionEncoding::Utf16,
        ) else {
            return self.reject(
                serial,
                ErrorCode::InvalidParams,
                "position is outside the document or splits a character",
            );
        };
        let generation = self.captures.generation;
        let capture = self.captures.latest.clone();
        if capture.is_none() && !self.captures.can_capture(generation) {
            return self.reject(
                serial,
                ErrorCode::RequestFailed,
                "input capture is unavailable",
            );
        }
        if let Some(token) = progress {
            let notification = match self.features.progress.begin(serial, token) {
                Ok(notification) => notification,
                Err(error) => {
                    return self.reject(serial, ErrorCode::InvalidParams, &error.to_string());
                }
            };
            self.outbox.push(notification.into(), None, 4096)?;
        }
        let uri = params.text_document.uri;
        let limit = self.limits.result_bytes;
        self.features.pending.push_back(FeatureTask {
            serial,
            generation,
            source_name: name.clone(),
            capture,
            dispatch: Box::new(move |actor, ctx, capture| {
                actor.start_feature::<Q>(
                    ctx,
                    FeatureInput {
                        serial,
                        capture,
                        documents,
                        source_name: name,
                        uri,
                        offset,
                        limit,
                    },
                )
            }),
        });
        // An interactive query may interrupt a cancellable background projection.
        if self.analysis.background {
            self.analysis.credit.cancel();
        }
        Ok(())
    }

    pub fn cancel_feature(&mut self, serial: u64) {
        self.features.cancel(serial);
        self.analysis.maintenance = true;
        self.captures.cancel_unneeded(&self.features);
    }

    pub fn fail_feature_capture(&mut self, generation: u64, message: &str) -> io::Result<()> {
        let failed: Vec<_> = self
            .features
            .pending
            .iter()
            .filter(|task| task.generation == generation)
            .map(|task| task.serial)
            .collect();
        for serial in failed {
            self.cancel_feature(serial);
            self.reject(serial, ErrorCode::RequestFailed, message)?;
        }
        Ok(())
    }

    pub fn schedule_feature(&mut self, ctx: &mut ActorContext<Self>) -> io::Result<bool> {
        let Some(mut task) = self.features.next_ready()? else {
            return Ok(false);
        };
        let capture = task.capture.take().expect("ready capture");
        (task.dispatch)(self, ctx, capture)?;
        Ok(true)
    }

    fn start_feature<Q: FeatureQuery>(
        &mut self,
        ctx: &mut ActorContext<Self>,
        input: FeatureInput,
    ) -> io::Result<()> {
        let token = self
            .requests
            .token(input.serial)
            .ok_or_else(|| Error::other("feature lost request ownership"))?;
        let job = self.analysis.credit.reserve(token.clone())?;
        let (sender, receiver) = oneshot::channel();
        let completion_input = input.clone();
        ctx.pipe_to_self(receiver, move |result| FeatureFinished::<Q> {
            job,
            input: completion_input,
            result,
            query: PhantomData,
        })
        .map_err(Error::other)?;
        self.features.active = Some(input.capture.clone());
        let mut retained = self.features.retained_contexts();
        retained.extend(self.analysis.live_contexts.iter().cloned());
        let live_serials = self.requests.unresolved().into_iter().collect();
        let input_generation = self.captures.generation;
        #[cfg(test)]
        let step_gate = self.features.step_gate.clone();
        #[cfg(all(test, feature = "verification"))]
        let gate = self.features.gate.take();
        #[cfg(all(test, feature = "verification"))]
        self.features
            .started
            .send_replace((job, input.capture.generation));
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
                    state.retain_input_generation(input_generation);
                    let result = Q::run(state, &input, cancel);
                    #[cfg(test)]
                    if matches!(result, Ok(QueryStep::Continue))
                        && let Some(gate) = step_gate
                    {
                        assert_eq!(state.index_cursor_count(), 1, "held real index cursor");
                        gate.hold(job, input.capture.generation);
                    }
                    result
                },
                token,
                sender,
            )
            .map_err(Error::other)?;
        Ok(())
    }

    fn finish_feature(&mut self, serial: u64, output: FeatureOutput) -> io::Result<()> {
        let reserve = self.requests.unresolved().len() * 3 * 4096;
        if !self.outbox.can_push_feature(output.charge, reserve) {
            return self.reject(
                serial,
                ErrorCode::RequestFailed,
                "semantic result exceeds available output capacity",
            );
        }
        self.finish_charged(serial, Ok(output.value), output.charge)
    }
}

impl<Q: FeatureQuery> Handler<FeatureFinished<Q>> for SessionActor {
    async fn handle(
        &mut self,
        _: &mut HandlerContext<'_, Self>,
        message: FeatureFinished<Q>,
    ) -> io::Result<()> {
        self.analysis.credit.complete(message.job)?;
        #[cfg(test)]
        self.analysis
            .completed
            .send_replace((message.job, message.input.capture.generation));
        self.features.active = None;
        if message
            .input
            .capture
            .contexts
            .iter()
            .any(|context| !self.analysis.live_contexts.contains(&context.key))
        {
            self.analysis.maintenance = true;
        }
        let result = message
            .result
            .map_err(|_| Error::other("analysis worker lost feature result"))?;
        let serial = message.input.serial;
        let Some(token) = self.requests.token(serial) else {
            return Ok(());
        };
        if self.state != Lifecycle::Running || token.check().is_err() {
            return Ok(());
        }
        match result {
            Ok(QueryStep::Complete(output)) => self.finish_feature(serial, output),
            Ok(QueryStep::Continue) => {
                self.features.progress.report(serial);
                let input = message.input;
                self.features.pending.push_back(FeatureTask {
                    serial,
                    generation: input.capture.generation,
                    source_name: input.source_name.clone(),
                    capture: Some(input.capture.clone()),
                    dispatch: Box::new(move |actor, ctx, _| actor.start_feature::<Q>(ctx, input)),
                });
                Ok(())
            }
            Err(AnalysisFailure::Cancelled) => {
                self.reject(serial, ErrorCode::RequestCancelled, "query cancelled")
            }
            Err(error) => self.reject(serial, ErrorCode::RequestFailed, &error.to_string()),
        }
    }
}

#[cfg(test)]
mod tests;

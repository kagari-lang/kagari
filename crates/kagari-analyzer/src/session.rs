//! Single owner of protocol state; Lattice dispatches concrete feature messages.
use std::io::Error;
pub(crate) mod analysis;
pub(crate) mod capture;
pub(crate) mod feature_scheduler;
pub(crate) mod handlers;
pub(crate) mod jobs;
pub(crate) mod outbox;
pub(crate) mod publications;
pub(crate) mod request_registry;
pub(crate) mod tasks;
pub(crate) mod timers;
#[cfg(any(test, feature = "verification"))]
pub(crate) mod verification;

use crate::{
    documents::Documents,
    logging::LogSink,
    protocol::{
        ErrorCode, Message as WireMessage, RequestId, Response, ResponseError, typed::encode_result,
    },
    server::{
        ProcessNotice, ThreadOwners,
        options::{Endpoint, SessionLimits},
    },
    session::{
        analysis::AnalysisHost,
        capture::CaptureState,
        feature_scheduler::FeatureScheduler,
        outbox::{Outbox, WriteFrame},
        publications::Publications,
        request_registry::{AdmissionError, RequestRegistry},
        timers::{Deadline, Timers},
    },
    transport::{self, IngressBudget},
};
use kagari_source::source_database::SourceDatabase;
use lattice_actor::{
    context::ActorContext,
    state_machine::Stateless,
    traits::{Actor, MessageMetadata, MessageOutcome},
};
use lsp_types::request::Request as LspRequest;
use serde_json::Value;
use std::{
    io,
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
        mpsc::SyncSender,
    },
    time::Instant,
};
use tokio::sync::{mpsc, watch};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Lifecycle {
    AwaitInitialize,
    AwaitInitialized,
    Running,
    ShuttingDown,
    AwaitExit,
    Stopping,
}

pub(crate) struct SessionActor {
    endpoint: Option<Endpoint>,
    pub state: Lifecycle,
    pub documents: Documents,
    pub captures: CaptureState,
    pub features: FeatureScheduler,
    pub analysis: AnalysisHost,
    pub publications: Publications,
    pub limits: SessionLimits,
    pub requests: RequestRegistry,
    pub outbox: Outbox,
    pub timers: Timers,
    timer_receiver: Option<watch::Receiver<Option<Instant>>>,
    pub writer: mpsc::Sender<WriteFrame>,
    pub output: Option<mpsc::Receiver<WriteFrame>>,
    pub budget: IngressBudget,
    pub stop: watch::Sender<bool>,
    pub logger: LogSink,
    pub threads: ThreadOwners,
    notices: SyncSender<ProcessNotice>,
    status: Arc<AtomicU8>,
    pub shutdown_serial: Option<u64>,
    pub shutdown_written: bool,
    pub exit_received: bool,
    writer_ready: bool,
}

impl SessionActor {
    pub fn new(
        endpoint: Endpoint,
        root: &str,
        limits: SessionLimits,
        logger: LogSink,
        threads: ThreadOwners,
        notices: SyncSender<ProcessNotice>,
        status: Arc<AtomicU8>,
    ) -> io::Result<Self> {
        let (writer, output) = mpsc::channel(1);
        let (stop, _) = watch::channel(false);
        let (timers, timer_receiver) = Timers::new();
        let documents = Documents::new(
            SourceDatabase::new(root).map_err(Error::other)?,
            limits.clone(),
        );
        Ok(Self {
            endpoint: Some(endpoint),
            state: Lifecycle::AwaitInitialize,
            documents,
            captures: CaptureState::new(root.to_owned(), limits.retained_input_bytes),
            analysis: AnalysisHost::new(),
            features: FeatureScheduler::default(),
            publications: Publications::new(&limits),
            requests: RequestRegistry::new(limits.requests),
            outbox: Outbox::new(limits.clone()),
            budget: IngressBudget::new(&limits),
            limits,
            timers,
            timer_receiver: Some(timer_receiver),
            writer,
            output: Some(output),
            stop,
            logger,
            threads,
            notices,
            status,
            shutdown_serial: None,
            shutdown_written: false,
            exit_received: false,
            writer_ready: false,
        })
    }

    pub fn admit(
        &mut self,
        ctx: &mut ActorContext<Self>,
        id: RequestId,
        method: &str,
    ) -> Option<u64> {
        let admitted = if matches!(method, "initialize" | "shutdown") {
            self.requests.admit_control(id.clone(), method.to_owned())
        } else {
            self.requests.admit(id.clone(), method.to_owned())
        };
        match admitted {
            Ok(serial) => Some(serial),
            Err(AdmissionError::Full) => {
                let result = self.outbox.push(
                    Response::new_err(
                        id,
                        ErrorCode::RequestFailed as i32,
                        "request capacity exhausted".into(),
                    )
                    .into(),
                    None,
                    4096,
                );
                if let Err(error) = result {
                    self.fail(ctx, error.to_string());
                }
                None
            }
            Err(error) => {
                self.fail(ctx, format!("request admission failed: {error:?}"));
                None
            }
        }
    }

    pub fn finish_lsp<R: LspRequest>(&mut self, serial: u64, result: R::Result) -> io::Result<()> {
        self.finish(
            serial,
            Ok(encode_result::<R>(result).map_err(Error::other)?),
        )
    }

    pub fn finish(&mut self, serial: u64, outcome: Result<Value, ResponseError>) -> io::Result<()> {
        self.finish_charged(serial, outcome, 4096)
    }

    pub fn finish_charged(
        &mut self,
        serial: u64,
        outcome: Result<Value, ResponseError>,
        charge: usize,
    ) -> io::Result<()> {
        if let Some(id) = self.requests.terminal(serial) {
            if let Some(notification) = self.features.progress.end(serial) {
                self.outbox.push(notification.into(), None, 4096)?;
            }
            self.outbox.push(
                WireMessage::Response(Response {
                    id: Some(id),
                    response_result: outcome,
                }),
                Some(serial),
                charge,
            )?;
        }
        Ok(())
    }

    pub fn reject(&mut self, serial: u64, code: ErrorCode, message: &str) -> io::Result<()> {
        self.finish(
            serial,
            Err(ResponseError {
                code: code as i32,
                message: message.chars().take(512).collect(),
                data: None,
            }),
        )
    }

    pub fn require_running(&mut self, serial: u64) -> io::Result<bool> {
        if self.state == Lifecycle::Running {
            return Ok(true);
        }
        let (code, message) = if matches!(
            self.state,
            Lifecycle::AwaitInitialize | Lifecycle::AwaitInitialized
        ) {
            (ErrorCode::ServerNotInitialized, "server is not initialized")
        } else {
            (ErrorCode::InvalidRequest, "server has shut down")
        };
        self.reject(serial, code, message)?;
        Ok(false)
    }

    pub fn fail(&mut self, ctx: &mut ActorContext<Self>, message: String) {
        if self.state == Lifecycle::Stopping {
            return;
        }
        self.logger.write(message);
        self.status.store(1, Ordering::Release);
        self.stop(ctx);
    }

    fn stop(&mut self, ctx: &mut ActorContext<Self>) {
        self.state = Lifecycle::Stopping;
        self.budget.close();
        self.stop.send_replace(true);
        self.features.shutdown();
        self.captures.shutdown();
        self.analysis.shutdown();
        self.publications.shutdown();
        for serial in self.requests.unresolved() {
            if let Some(token) = self.requests.token(serial) {
                token.cancel();
            }
        }
        let _ = self.notices.try_send(ProcessNotice::Cleanup);
        ctx.request_stop();
    }

    pub fn begin_exit(&mut self) {
        self.exit_received = true;
        self.timers
            .set(Deadline::Cleanup, Instant::now() + self.limits.cleanup);
        let _ = self.notices.try_send(ProcessNotice::Cleanup);
    }

    fn schedule(&mut self, ctx: &mut ActorContext<Self>) -> io::Result<()> {
        if self.state == Lifecycle::Stopping {
            return Ok(());
        }
        self.schedule_capture(ctx)?;
        self.schedule_analysis(ctx)?;
        if self.writer_ready && self.outbox.active.is_none() {
            let optional_capacity = self.outbox.can_grant_publication()
                && self.outbox.can_push_feature(
                    self.limits.result_bytes,
                    self.requests.unresolved().len() * 3 * 4096,
                );
            let report = if !self.outbox.has_queued() && optional_capacity {
                self.features.progress.take_report()
            } else {
                None
            };
            let frame = if let Some((serial, report)) = report {
                Some(self.outbox.grant_progress(report.into(), serial)?)
            } else if optional_capacity
                && let Some(publication) = self.publications.next(self.outbox.has_queued())?
            {
                Some(self.outbox.grant_publication(
                    publication.notification.into(),
                    publication.charge,
                    publication.publication_serial,
                )?)
            } else {
                let frame = self.outbox.grant();
                if frame.is_some() {
                    self.publications.reliable_granted();
                }
                frame
            };
            if let Some(frame) = frame {
                self.timers
                    .set(Deadline::Write, Instant::now() + self.limits.write);
                self.writer
                    .try_send(frame)
                    .map_err(|_| Error::other("writer credit invariant violated"))?;
            }
        }
        if self.exit_received
            && self.shutdown_written
            && self.outbox.is_empty()
            && self.captures.is_idle()
            && self.analysis.credit.is_idle()
        {
            self.status.store(0, Ordering::Release);
            self.stop(ctx);
        }
        Ok(())
    }
}

impl Actor for SessionActor {
    type Error = io::Error;
    type Behavior = Stateless;

    async fn started(&mut self, ctx: &mut ActorContext<Self>) -> io::Result<()> {
        timers::start(ctx, self.timer_receiver.take().expect("single startup"));
        self.timers
            .set(Deadline::Initialize, Instant::now() + self.limits.startup);
        transport::prepare(
            ctx,
            self.endpoint.take().expect("single endpoint startup"),
            self.logger.clone(),
            &self.limits,
        )
    }

    fn after_message(
        &mut self,
        ctx: &mut ActorContext<Self>,
        _: &MessageMetadata,
        outcome: MessageOutcome,
    ) {
        if outcome != MessageOutcome::Handled {
            self.fail(ctx, format!("session dispatch failed: {outcome:?}"));
            return;
        }
        if let Err(error) = self.schedule(ctx) {
            self.fail(ctx, error.to_string());
        }
    }
}

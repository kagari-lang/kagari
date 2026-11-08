//! Lifecycle messages are ordinary typed Lattice handlers, including invalid phases.
use crate::{
    protocol::{
        ErrorCode,
        typed::{TypedNotification, TypedRequest},
    },
    session::{Lifecycle, SessionActor, handlers::initialize_response, timers::Deadline},
};
use lattice_actor::{context::HandlerContext, traits::Handler};
use lsp_types::{
    notification::{Exit, Initialized as InitializedMethod},
    request::{Initialize, Request as LspRequest, Shutdown},
};
use std::io::{self, Error};

#[derive(lattice_actor::Message)]
pub(crate) struct InitializeRequested(pub TypedRequest<Initialize>);

#[derive(lattice_actor::Message)]
pub(crate) struct Initialized(pub TypedNotification<InitializedMethod>);

#[derive(lattice_actor::Message)]
pub(crate) struct ShutdownRequested(pub TypedRequest<Shutdown>);

#[derive(lattice_actor::Message)]
pub(crate) struct ExitRequested(pub TypedNotification<Exit>);

impl Handler<InitializeRequested> for SessionActor {
    async fn handle(
        &mut self,
        ctx: &mut HandlerContext<'_, Self>,
        message: InitializeRequested,
    ) -> io::Result<()> {
        let request = message.0;
        let Some(serial) = self.admit(ctx, request.id, Initialize::METHOD) else {
            return Ok(());
        };
        if self.state != Lifecycle::AwaitInitialize {
            return self.reject(
                serial,
                ErrorCode::InvalidRequest,
                "server is already initialized",
            );
        }
        let params = match request.params {
            Ok(params) => params,
            Err(_) => {
                return self.reject(
                    serial,
                    ErrorCode::InvalidParams,
                    "invalid initialize parameters",
                );
            }
        };
        if let Err(error) = self.captures.configure(&params) {
            return self.reject(serial, ErrorCode::InvalidParams, &error);
        }
        self.progress.supported = params
            .capabilities
            .window
            .as_ref()
            .and_then(|window| window.work_done_progress)
            .unwrap_or(false);
        self.publications.set_version_support(
            params
                .capabilities
                .text_document
                .as_ref()
                .and_then(|capabilities| capabilities.publish_diagnostics.as_ref())
                .and_then(|capabilities| capabilities.version_support)
                .unwrap_or(false),
        );
        self.state = Lifecycle::AwaitInitialized;
        let result = initialize_response().map_err(Error::other)?;
        self.finish(serial, Ok(result))
    }
}

impl Handler<Initialized> for SessionActor {
    async fn handle(
        &mut self,
        ctx: &mut HandlerContext<'_, Self>,
        message: Initialized,
    ) -> io::Result<()> {
        if self.state == Lifecycle::AwaitInitialized && message.0.params.is_ok() {
            self.state = Lifecycle::Running;
            self.timers.remove(Deadline::Initialize);
            self.start_acquisition(ctx)?;
            self.start_analysis(ctx)?;
        }
        Ok(())
    }
}

impl Handler<ShutdownRequested> for SessionActor {
    async fn handle(
        &mut self,
        ctx: &mut HandlerContext<'_, Self>,
        message: ShutdownRequested,
    ) -> io::Result<()> {
        let request = message.0;
        let Some(serial) = self.admit(ctx, request.id, Shutdown::METHOD) else {
            return Ok(());
        };
        if !self.require_running(serial)? {
            return Ok(());
        }
        if request.params.is_err() {
            return self.reject(
                serial,
                ErrorCode::InvalidParams,
                "shutdown requires null parameters",
            );
        }
        self.state = Lifecycle::ShuttingDown;
        self.operations.shutdown();
        self.captures.shutdown();
        self.analysis.shutdown();
        self.diagnostics.shutdown();
        self.publications.shutdown();
        self.timers.remove(Deadline::Diagnostics);
        for pending in self.requests.unresolved() {
            if pending != serial {
                if let Some(token) = self.requests.token(pending) {
                    token.cancel();
                }
                self.reject(pending, ErrorCode::RequestFailed, "server is shutting down")?;
            }
        }
        self.shutdown_serial = Some(serial);
        self.finish_lsp::<Shutdown>(serial, ())
    }
}

impl Handler<ExitRequested> for SessionActor {
    async fn handle(
        &mut self,
        ctx: &mut HandlerContext<'_, Self>,
        message: ExitRequested,
    ) -> io::Result<()> {
        if message.0.params.is_err()
            || !matches!(self.state, Lifecycle::ShuttingDown | Lifecycle::AwaitExit)
        {
            self.fail(
                ctx,
                "exit requires accepted shutdown and null parameters".into(),
            );
        } else {
            self.begin_exit();
        }
        Ok(())
    }
}

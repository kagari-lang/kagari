//! Lifecycle messages are ordinary typed Lattice handlers, including invalid phases.
use crate::{
    protocol::{ErrorCode, Notification, Request},
    session::{Lifecycle, SessionActor, timers::Deadline},
};
use lattice_actor::{
    context::HandlerContext,
    traits::{Handler, Message},
};
use lsp_types::{
    InitializeParams, InitializeResult, InitializedParams, PositionEncodingKind,
    ServerCapabilities, ServerInfo, TextDocumentSyncKind, TextDocumentSyncOptions,
};
use serde_json::{Value, from_value, to_value};
use std::io::{self, Error};

pub(crate) struct InitializeRequested(pub Request);

pub(crate) struct Initialized(pub Notification);

pub(crate) struct ShutdownRequested(pub Request);

pub(crate) struct ExitRequested(pub Notification);

impl Message for InitializeRequested {}

impl Message for Initialized {}

impl Message for ShutdownRequested {}

impl Message for ExitRequested {}

impl Handler<InitializeRequested> for SessionActor {
    async fn handle(
        &mut self,
        ctx: &mut HandlerContext<'_, Self>,
        message: InitializeRequested,
    ) -> io::Result<()> {
        let request = message.0;
        let Some(serial) = self.admit(ctx, request.id, "initialize") else {
            return Ok(());
        };
        if self.state != Lifecycle::AwaitInitialize {
            return self.reject(
                serial,
                ErrorCode::InvalidRequest,
                "server is already initialized",
            );
        }
        let params = from_value::<InitializeParams>(request.params);
        if params.is_err() {
            return self.reject(
                serial,
                ErrorCode::InvalidParams,
                "invalid initialize parameters",
            );
        }
        self.state = Lifecycle::AwaitInitialized;
        self.finish(serial, Ok(to_value(capabilities()).map_err(Error::other)?))
    }
}

impl Handler<Initialized> for SessionActor {
    async fn handle(
        &mut self,
        _: &mut HandlerContext<'_, Self>,
        message: Initialized,
    ) -> io::Result<()> {
        if self.state == Lifecycle::AwaitInitialized
            && from_value::<InitializedParams>(message.0.params).is_ok()
        {
            self.state = Lifecycle::Running;
            self.timers.remove(Deadline::Initialize);
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
        let Some(serial) = self.admit(ctx, request.id, "shutdown") else {
            return Ok(());
        };
        if !self.require_running(serial)? {
            return Ok(());
        }
        if !request.params.is_null() {
            return self.reject(
                serial,
                ErrorCode::InvalidParams,
                "shutdown requires null parameters",
            );
        }
        self.state = Lifecycle::ShuttingDown;
        for pending in self.requests.unresolved() {
            if pending != serial {
                if let Some(token) = self.requests.token(pending) {
                    token.cancel();
                }
                self.reject(pending, ErrorCode::RequestFailed, "server is shutting down")?;
            }
        }
        self.shutdown_serial = Some(serial);
        self.finish(serial, Ok(Value::Null))
    }
}

impl Handler<ExitRequested> for SessionActor {
    async fn handle(
        &mut self,
        ctx: &mut HandlerContext<'_, Self>,
        message: ExitRequested,
    ) -> io::Result<()> {
        if !message.0.params.is_null()
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

fn capabilities() -> InitializeResult {
    InitializeResult {
        capabilities: ServerCapabilities {
            position_encoding: Some(PositionEncodingKind::UTF16),
            text_document_sync: Some(
                TextDocumentSyncOptions {
                    open_close: Some(true),
                    change: Some(TextDocumentSyncKind::FULL),
                    ..Default::default()
                }
                .into(),
            ),
            ..Default::default()
        },
        server_info: Some(ServerInfo {
            name: "kagari-analyzer".into(),
            version: Some(env!("CARGO_PKG_VERSION").into()),
        }),
    }
}

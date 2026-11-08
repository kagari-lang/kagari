//! Request policy shared by unsupported methods and client cancellation.
use crate::{
    protocol::{
        ErrorCode, MAX_REQUEST_ID_BYTES, Notification, Request, RequestId, Response,
        typed::TypedNotification,
    },
    session::SessionActor,
};
use lattice_actor::{context::HandlerContext, traits::Handler};
use lsp_types::{
    NumberOrString, ProgressToken,
    notification::{Cancel, WorkDoneProgressCancel},
};

use std::io;

#[derive(lattice_actor::Message)]
pub(crate) struct UnknownRequest(pub Request);

#[derive(lattice_actor::Message)]
pub(crate) struct UnknownNotification(pub Notification);

#[derive(lattice_actor::Message)]
pub(crate) struct ClientResponse(pub Response);

#[derive(lattice_actor::Message)]
pub(crate) struct CancelRequested(pub TypedNotification<Cancel>);

impl Handler<UnknownRequest> for SessionActor {
    async fn handle(
        &mut self,
        ctx: &mut HandlerContext<'_, Self>,
        message: UnknownRequest,
    ) -> io::Result<()> {
        let Some(serial) = self.admit(ctx, message.0.id, "unsupported") else {
            return Ok(());
        };
        if self.require_running(serial)? {
            self.reject(serial, ErrorCode::MethodNotFound, "unsupported request")?;
        }
        Ok(())
    }
}

impl Handler<UnknownNotification> for SessionActor {
    async fn handle(
        &mut self,
        _: &mut HandlerContext<'_, Self>,
        message: UnknownNotification,
    ) -> io::Result<()> {
        if !message.0.method.starts_with("$/") {
            self.logger.write("ignored unsupported notification".into());
        }
        Ok(())
    }
}

impl Handler<ClientResponse> for SessionActor {
    async fn handle(
        &mut self,
        _: &mut HandlerContext<'_, Self>,
        message: ClientResponse,
    ) -> io::Result<()> {
        if message.0.id.is_some() {
            self.logger.write("ignored unknown client response".into());
        }
        Ok(())
    }
}

impl Handler<CancelRequested> for SessionActor {
    async fn handle(
        &mut self,
        _: &mut HandlerContext<'_, Self>,
        message: CancelRequested,
    ) -> io::Result<()> {
        let Ok(params) = message.0.params else {
            return Ok(());
        };
        let id = match params.id {
            NumberOrString::Number(id) => Some(RequestId::Integer(id)),
            NumberOrString::String(id) if id.len() <= MAX_REQUEST_ID_BYTES => {
                Some(RequestId::String(id))
            }
            _ => None,
        };
        if let Some(serial) = id.as_ref().and_then(|id| self.requests.cancel(id)) {
            if let Some(method) = self.requests.method(serial) {
                self.logger.write(format!("cancelled {method}"));
            }
            self.cancel_request_work(serial);
            self.reject(serial, ErrorCode::RequestCancelled, "request cancelled")?;
        }
        Ok(())
    }
}

/// Work-done cancellation addresses the active progress token, not the request ID.
#[derive(lattice_actor::Message)]
pub(crate) struct ProgressCancelled(pub TypedNotification<WorkDoneProgressCancel>);

impl Handler<ProgressCancelled> for SessionActor {
    async fn handle(
        &mut self,
        _: &mut HandlerContext<'_, Self>,
        message: ProgressCancelled,
    ) -> io::Result<()> {
        let Ok(params) = message.0.params else {
            return Ok(());
        };
        if matches!(&params.token, ProgressToken::String(token) if token.len() > 256) {
            return Ok(());
        }
        if let Some(serial) = self.progress.serial(&params.token)
            && self.requests.cancel_serial(serial)
        {
            self.cancel_request_work(serial);
            self.reject(
                serial,
                ErrorCode::RequestCancelled,
                "request cancelled through work-done progress",
            )?;
        }
        Ok(())
    }
}

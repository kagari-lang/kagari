//! Request policy shared by unsupported methods and client cancellation.
use crate::{
    protocol::{ErrorCode, Notification, Request, RequestId, Response},
    session::SessionActor,
};
use lattice_actor::{
    context::HandlerContext,
    traits::{Handler, Message},
};
use serde_json::Value;
use std::io;

pub(crate) struct UnknownRequest(pub Request);

pub(crate) struct UnknownNotification(pub Notification);

pub(crate) struct ClientResponse(pub Response);

pub(crate) struct CancelRequested(pub Notification);

impl Message for UnknownRequest {}

impl Message for UnknownNotification {}

impl Message for ClientResponse {}

impl Message for CancelRequested {}

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
        let id = match message.0.params.get("id") {
            Some(Value::Number(value)) => value
                .as_i64()
                .and_then(|id| i32::try_from(id).ok())
                .map(RequestId::Integer),
            Some(Value::String(value)) => Some(RequestId::String(value.clone())),
            _ => None,
        };
        if let Some(serial) = id.as_ref().and_then(|id| self.requests.cancel(id)) {
            if let Some(method) = self.requests.method(serial) {
                self.logger.write(format!("cancelled {method}"));
            }
            self.reject(serial, ErrorCode::RequestCancelled, "request cancelled")?;
        }
        Ok(())
    }
}

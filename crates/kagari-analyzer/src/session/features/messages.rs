//! Each protocol method has a concrete Lattice ingress type.
use crate::{
    analysis::features::{CompletionQuery, DefinitionQuery, HoverQuery, ImplementationQuery},
    protocol::{ErrorCode, Notification, Request},
    session::SessionActor,
};
use lattice_actor::{
    context::HandlerContext,
    traits::{Handler, Message},
};
use lsp_types::{ProgressToken, WorkDoneProgressCancelParams};
use serde_json::from_value;
use std::io;

pub(crate) struct HoverRequested(pub Request);

pub(crate) struct DefinitionRequested(pub Request);

pub(crate) struct ImplementationRequested(pub Request);

pub(crate) struct CompletionRequested(pub Request);

impl Message for HoverRequested {}

impl Message for DefinitionRequested {}

impl Message for ImplementationRequested {}

impl Message for CompletionRequested {}

impl Handler<HoverRequested> for SessionActor {
    async fn handle(
        &mut self,
        ctx: &mut HandlerContext<'_, Self>,
        message: HoverRequested,
    ) -> io::Result<()> {
        self.admit_feature::<HoverQuery>(ctx, message.0)
    }
}

impl Handler<DefinitionRequested> for SessionActor {
    async fn handle(
        &mut self,
        ctx: &mut HandlerContext<'_, Self>,
        message: DefinitionRequested,
    ) -> io::Result<()> {
        self.admit_feature::<DefinitionQuery>(ctx, message.0)
    }
}

impl Handler<ImplementationRequested> for SessionActor {
    async fn handle(
        &mut self,
        ctx: &mut HandlerContext<'_, Self>,
        message: ImplementationRequested,
    ) -> io::Result<()> {
        self.admit_feature::<ImplementationQuery>(ctx, message.0)
    }
}

impl Handler<CompletionRequested> for SessionActor {
    async fn handle(
        &mut self,
        ctx: &mut HandlerContext<'_, Self>,
        message: CompletionRequested,
    ) -> io::Result<()> {
        self.admit_feature::<CompletionQuery>(ctx, message.0)
    }
}

/// Work-done cancellation addresses the active progress token, not the request ID.
pub(crate) struct ProgressCancelled(pub Notification);

impl Message for ProgressCancelled {}

impl Handler<ProgressCancelled> for SessionActor {
    async fn handle(
        &mut self,
        _: &mut HandlerContext<'_, Self>,
        message: ProgressCancelled,
    ) -> io::Result<()> {
        let Ok(params) = from_value::<WorkDoneProgressCancelParams>(message.0.params) else {
            return Ok(());
        };
        if matches!(&params.token, ProgressToken::String(token) if token.len() > 256) {
            return Ok(());
        }
        if let Some(serial) = self.features.progress.serial(&params.token)
            && self.requests.cancel_serial(serial)
        {
            self.cancel_feature(serial);
            self.reject(
                serial,
                ErrorCode::RequestCancelled,
                "request cancelled through work-done progress",
            )?;
        }
        Ok(())
    }
}

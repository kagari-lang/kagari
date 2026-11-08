//! Ordered document changes; source storage retains the text and coordinate authority.
use crate::{
    protocol::{ErrorCode, Notification, Request},
    session::{Lifecycle, SessionActor},
};
use lattice_actor::{context::HandlerContext, traits::Handler};
use lsp_types::{DidChangeWatchedFilesParams, DidSaveTextDocumentParams, ExecuteCommandParams};
use serde_json::Value;
use serde_json::from_value;
use std::io;

#[derive(lattice_actor::Message)]
pub(crate) struct Opened(pub Notification);

#[derive(lattice_actor::Message)]
pub(crate) struct Changed(pub Notification);

#[derive(lattice_actor::Message)]
pub(crate) struct Closed(pub Notification);

#[derive(lattice_actor::Message)]
pub(crate) struct Saved(pub Notification);

#[derive(lattice_actor::Message)]
pub(crate) struct WatchedFilesChanged(pub Notification);

#[derive(lattice_actor::Message)]
pub(crate) struct ReloadRequested(pub Request);

impl Handler<Opened> for SessionActor {
    async fn handle(
        &mut self,
        ctx: &mut HandlerContext<'_, Self>,
        message: Opened,
    ) -> io::Result<()> {
        if self.state == Lifecycle::Running {
            let result = from_value(message.0.params)
                .map_err(|error| error.to_string())
                .and_then(|params| self.documents.open(params));
            if let Err(error) = result {
                self.logger.write(error);
            }
            if self.documents.synchronization_failed() {
                self.fail(
                    ctx,
                    "document metadata capacity exhausted; synchronization lost".into(),
                );
            } else {
                self.invalidate_inputs()?;
            }
        }
        Ok(())
    }
}

impl Handler<Changed> for SessionActor {
    async fn handle(
        &mut self,
        ctx: &mut HandlerContext<'_, Self>,
        message: Changed,
    ) -> io::Result<()> {
        if self.state == Lifecycle::Running {
            let result = from_value(message.0.params)
                .map_err(|error| error.to_string())
                .and_then(|params| self.documents.change(params));
            if let Err(error) = result {
                self.logger.write(error);
            }
            if self.documents.synchronization_failed() {
                self.fail(
                    ctx,
                    "document metadata capacity exhausted; synchronization lost".into(),
                );
            } else {
                self.invalidate_inputs()?;
            }
        }
        Ok(())
    }
}

impl Handler<Closed> for SessionActor {
    async fn handle(
        &mut self,
        ctx: &mut HandlerContext<'_, Self>,
        message: Closed,
    ) -> io::Result<()> {
        if self.state == Lifecycle::Running {
            let result = from_value(message.0.params)
                .map_err(|error| error.to_string())
                .and_then(|params| self.documents.close(params));
            if let Err(error) = result {
                self.logger.write(error);
            }
            if self.documents.synchronization_failed() {
                self.fail(
                    ctx,
                    "document metadata capacity exhausted; synchronization lost".into(),
                );
            } else {
                self.invalidate_inputs()?;
            }
        }
        Ok(())
    }
}

impl Handler<Saved> for SessionActor {
    async fn handle(&mut self, _: &mut HandlerContext<'_, Self>, message: Saved) -> io::Result<()> {
        if self.state == Lifecycle::Running
            && from_value::<DidSaveTextDocumentParams>(message.0.params).is_ok()
        {
            // The authoritative unsaved buffer is changed only by didChange.
            self.invalidate_inputs()?;
        }
        Ok(())
    }
}

impl Handler<WatchedFilesChanged> for SessionActor {
    async fn handle(
        &mut self,
        _: &mut HandlerContext<'_, Self>,
        message: WatchedFilesChanged,
    ) -> io::Result<()> {
        if self.state == Lifecycle::Running
            && from_value::<DidChangeWatchedFilesParams>(message.0.params).is_ok()
        {
            self.invalidate_inputs()?;
        }
        Ok(())
    }
}

impl Handler<ReloadRequested> for SessionActor {
    async fn handle(
        &mut self,
        ctx: &mut HandlerContext<'_, Self>,
        message: ReloadRequested,
    ) -> io::Result<()> {
        let Some(serial) = self.admit(ctx, message.0.id, "workspace/executeCommand") else {
            return Ok(());
        };
        if !self.require_running(serial)? {
            return Ok(());
        }
        let Ok(params) = from_value::<ExecuteCommandParams>(message.0.params) else {
            return self.reject(
                serial,
                ErrorCode::InvalidParams,
                "invalid command parameters",
            );
        };
        if params.command != "kagari.reloadWorkspace" || !params.arguments.is_empty() {
            return self.reject(
                serial,
                ErrorCode::InvalidParams,
                "unsupported command or arguments",
            );
        }
        self.invalidate_inputs()?;
        // Null acknowledges scheduling; semantic reads use the resulting input barrier.
        self.finish(serial, Ok(Value::Null))
    }
}

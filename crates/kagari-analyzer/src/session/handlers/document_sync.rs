//! Ordered document changes; source storage retains the text and coordinate authority.
use crate::{
    protocol::{
        ErrorCode,
        typed::{TypedNotification, TypedRequest},
    },
    session::{Lifecycle, SessionActor},
};
use lattice_actor::{context::HandlerContext, traits::Handler};
use lsp_types::{
    ExecuteCommandOptions, ServerCapabilities, TextDocumentSyncKind, TextDocumentSyncOptions,
    notification::{
        DidChangeTextDocument, DidChangeWatchedFiles, DidCloseTextDocument, DidOpenTextDocument,
        DidSaveTextDocument,
    },
    request::{ExecuteCommand, Request as LspRequest},
};
use std::io;

#[derive(lattice_actor::Message)]
pub(crate) struct Opened(pub TypedNotification<DidOpenTextDocument>);

#[derive(lattice_actor::Message)]
pub(crate) struct Changed(pub TypedNotification<DidChangeTextDocument>);

#[derive(lattice_actor::Message)]
pub(crate) struct Closed(pub TypedNotification<DidCloseTextDocument>);

#[derive(lattice_actor::Message)]
pub(crate) struct Saved(pub TypedNotification<DidSaveTextDocument>);

#[derive(lattice_actor::Message)]
pub(crate) struct WatchedFilesChanged(pub TypedNotification<DidChangeWatchedFiles>);

#[derive(lattice_actor::Message)]
pub(crate) struct ReloadRequested(pub TypedRequest<ExecuteCommand>);

impl Handler<Opened> for SessionActor {
    async fn handle(
        &mut self,
        ctx: &mut HandlerContext<'_, Self>,
        message: Opened,
    ) -> io::Result<()> {
        if self.state == Lifecycle::Running {
            let result = message
                .0
                .params
                .map_err(|_| "invalid document notification parameters".to_owned())
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
            let result = message
                .0
                .params
                .map_err(|_| "invalid document notification parameters".to_owned())
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
            let result = message
                .0
                .params
                .map_err(|_| "invalid document notification parameters".to_owned())
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
        if self.state == Lifecycle::Running && message.0.params.is_ok() {
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
        if self.state == Lifecycle::Running && message.0.params.is_ok() {
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
        let Some(serial) = self.admit(ctx, message.0.id, ExecuteCommand::METHOD) else {
            return Ok(());
        };
        if !self.require_running(serial)? {
            return Ok(());
        }
        let Ok(params) = message.0.params else {
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
        self.finish_lsp::<ExecuteCommand>(serial, None)
    }
}

pub(crate) fn capabilities(capabilities: &mut ServerCapabilities) {
    capabilities.text_document_sync = Some(
        TextDocumentSyncOptions {
            open_close: Some(true),
            change: Some(TextDocumentSyncKind::FULL),
            save: Some(true.into()),
            ..Default::default()
        }
        .into(),
    );
    capabilities.execute_command_provider = Some(ExecuteCommandOptions {
        commands: vec!["kagari.reloadWorkspace".into()],
        ..Default::default()
    });
}

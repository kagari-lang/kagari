//! Source-owned coordinate preparation; no feature dispatch or progress policy.
use crate::{
    analysis::features::DocumentQueryInput,
    documents::DocumentInput,
    protocol::{ErrorCode, source_name},
    session::SessionActor,
    workspace::WorkspaceCapture,
};
use kagari_source::line_index::{Position, PositionEncoding};
use lattice_actor::context::ActorContext;
use lsp_types::{TextDocumentPositionParams, Uri};
use std::{io, sync::Arc};

pub(crate) struct DocumentPreparation {
    serial: u64,
    generation: u64,
    documents: Arc<[DocumentInput]>,
    source_name: String,
    uri: Uri,
    offset: usize,
    limit: usize,
    capture: Option<Arc<WorkspaceCapture>>,
}

impl SessionActor {
    pub fn prepare_document(
        &mut self,
        serial: u64,
        position: &TextDocumentPositionParams,
    ) -> io::Result<Option<DocumentPreparation>> {
        let name = match source_name(&position.text_document.uri) {
            Ok(name) => name,
            Err(error) => {
                self.reject(serial, ErrorCode::InvalidParams, &error)?;
                return Ok(None);
            }
        };
        let documents: Arc<[_]> = self.documents.inputs().into();
        let Some(document) = documents
            .iter()
            .find(|document| document.source_name == name)
        else {
            self.reject(
                serial,
                ErrorCode::InvalidParams,
                "query document is not open",
            )?;
            return Ok(None);
        };
        let Some(source) = document.source.as_ref() else {
            self.reject(
                serial,
                ErrorCode::RequestFailed,
                "query document text is unavailable",
            )?;
            return Ok(None);
        };
        let Some(offset) = source.offset(
            Position {
                line: position.position.line as usize,
                character: position.position.character as usize,
            },
            PositionEncoding::Utf16,
        ) else {
            self.reject(
                serial,
                ErrorCode::InvalidParams,
                "position is outside the document or splits a character",
            )?;
            return Ok(None);
        };
        let generation = self.captures.generation;
        let capture = self.captures.latest.clone();
        if capture.is_none() && !self.captures.can_capture(generation) {
            self.reject(
                serial,
                ErrorCode::RequestFailed,
                "input capture is unavailable",
            )?;
            return Ok(None);
        }
        Ok(Some(DocumentPreparation {
            serial,
            generation,
            documents,
            source_name: name,
            uri: position.text_document.uri.clone(),
            offset,
            limit: self.limits.result_bytes,
            capture,
        }))
    }

    pub fn wait_for_document(
        &mut self,
        preparation: DocumentPreparation,
        ready: impl FnOnce(
            &mut SessionActor,
            &mut ActorContext<SessionActor>,
            DocumentQueryInput,
        ) -> io::Result<()>
        + Send
        + 'static,
    ) {
        self.enqueue_request(
            preparation.serial,
            preparation.generation,
            preparation.source_name.clone(),
            preparation.capture,
            Box::new(move |actor, ctx, capture| {
                ready(
                    actor,
                    ctx,
                    DocumentQueryInput {
                        serial: preparation.serial,
                        capture,
                        documents: preparation.documents,
                        source_name: preparation.source_name,
                        uri: preparation.uri,
                        offset: preparation.offset,
                        limit: preparation.limit,
                    },
                )
            }),
        );
    }

    pub fn requeue_document(
        &mut self,
        input: DocumentQueryInput,
        ready: impl FnOnce(
            &mut SessionActor,
            &mut ActorContext<SessionActor>,
            DocumentQueryInput,
        ) -> io::Result<()>
        + Send
        + 'static,
    ) {
        self.enqueue_request(
            input.serial,
            input.capture.generation,
            input.source_name.clone(),
            Some(input.capture.clone()),
            Box::new(move |actor, ctx, _| ready(actor, ctx, input)),
        );
    }
}

//! Completion owns parameter interpretation, submitted work and its completion.
use crate::{
    analysis::{
        AnalysisFailure,
        features::{DocumentQueryInput, ProtocolOutput},
    },
    protocol::typed::TypedRequest,
    session::{SessionActor, request_tasks::WorkCompletion},
};
use lattice_actor::{
    context::{ActorContext, HandlerContext},
    traits::Handler,
};
use lsp_types::{CompletionOptions, ServerCapabilities, request::Completion};
use std::io;

#[derive(lattice_actor::Message)]
pub(crate) struct CompletionRequested(pub TypedRequest<Completion>);

#[derive(lattice_actor::Message)]
struct CompletionFinished(WorkCompletion<Result<ProtocolOutput, AnalysisFailure>>);

impl Handler<CompletionRequested> for SessionActor {
    async fn handle(
        &mut self,
        ctx: &mut HandlerContext<'_, Self>,
        message: CompletionRequested,
    ) -> io::Result<()> {
        let Some(request) = self.admit_read(ctx, message.0)? else {
            return Ok(());
        };
        let Some(input) =
            self.prepare_document(request.serial, &request.params.text_document_position)?
        else {
            return Ok(());
        };
        self.wait_for_document(input, |actor, ctx, input| {
            actor.start_completion(ctx, input)
        });
        Ok(())
    }
}

impl SessionActor {
    fn start_completion(
        &mut self,
        ctx: &mut ActorContext<Self>,
        input: DocumentQueryInput,
    ) -> io::Result<()> {
        self.submit_request_work(
            ctx,
            input.serial,
            input.capture.clone(),
            move |state, cancel, _job| {
                let result = state.completion(&input, cancel)?;
                ProtocolOutput::for_request::<Completion>(result, input.limit)
            },
            CompletionFinished,
        )
    }
}

impl Handler<CompletionFinished> for SessionActor {
    async fn handle(
        &mut self,
        _: &mut HandlerContext<'_, Self>,
        message: CompletionFinished,
    ) -> io::Result<()> {
        let serial = message.0.serial;
        if let Some(result) = self.accept_request_work(message.0)? {
            self.respond_request(serial, result)?;
        }
        Ok(())
    }
}

pub(crate) fn capabilities(capabilities: &mut ServerCapabilities) {
    capabilities.completion_provider = Some(CompletionOptions {
        trigger_characters: Some(vec![".".into(), ":".into()]),
        resolve_provider: Some(false),
        ..Default::default()
    });
}

//! Definition owns parameter interpretation, submitted work and its completion.
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
use lsp_types::{OneOf, ServerCapabilities, request::GotoDefinition};
use std::io;

#[derive(lattice_actor::Message)]
pub(crate) struct DefinitionRequested(pub TypedRequest<GotoDefinition>);

#[derive(lattice_actor::Message)]
struct DefinitionFinished(WorkCompletion<Result<ProtocolOutput, AnalysisFailure>>);

impl Handler<DefinitionRequested> for SessionActor {
    async fn handle(
        &mut self,
        ctx: &mut HandlerContext<'_, Self>,
        message: DefinitionRequested,
    ) -> io::Result<()> {
        let Some(request) = self.admit_read(ctx, message.0)? else {
            return Ok(());
        };
        let Some(input) = self.prepare_document(
            request.serial,
            &request.params.text_document_position_params,
        )?
        else {
            return Ok(());
        };
        self.wait_for_document(input, |actor, ctx, input| {
            actor.start_definition(ctx, input)
        });
        Ok(())
    }
}

impl SessionActor {
    fn start_definition(
        &mut self,
        ctx: &mut ActorContext<Self>,
        input: DocumentQueryInput,
    ) -> io::Result<()> {
        self.submit_request_work(
            ctx,
            input.serial,
            input.capture.clone(),
            move |state, cancel, _job| {
                let result = state.definition(&input, cancel)?;
                ProtocolOutput::for_request::<GotoDefinition>(result, input.limit)
            },
            DefinitionFinished,
        )
    }
}

impl Handler<DefinitionFinished> for SessionActor {
    async fn handle(
        &mut self,
        _: &mut HandlerContext<'_, Self>,
        message: DefinitionFinished,
    ) -> io::Result<()> {
        let serial = message.0.serial;
        if let Some(result) = self.accept_request_work(message.0)? {
            self.respond_request(serial, result)?;
        }
        Ok(())
    }
}

pub(crate) fn capabilities(capabilities: &mut ServerCapabilities) {
    capabilities.definition_provider = Some(OneOf::Left(true));
}

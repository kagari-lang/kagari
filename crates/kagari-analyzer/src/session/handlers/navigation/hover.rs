//! Hover owns parameter interpretation, submitted work and its completion.
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
use lsp_types::{HoverProviderCapability, ServerCapabilities, request::HoverRequest};
use std::io;

#[derive(lattice_actor::Message)]
pub(crate) struct HoverRequested(pub TypedRequest<HoverRequest>);

#[derive(lattice_actor::Message)]
struct HoverFinished(WorkCompletion<Result<ProtocolOutput, AnalysisFailure>>);

impl Handler<HoverRequested> for SessionActor {
    async fn handle(
        &mut self,
        ctx: &mut HandlerContext<'_, Self>,
        message: HoverRequested,
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
        self.wait_for_document(input, |actor, ctx, input| actor.start_hover(ctx, input));
        Ok(())
    }
}

impl SessionActor {
    fn start_hover(
        &mut self,
        ctx: &mut ActorContext<Self>,
        input: DocumentQueryInput,
    ) -> io::Result<()> {
        self.submit_request_work(
            ctx,
            input.serial,
            input.capture.clone(),
            move |state, cancel, _job| {
                let result = state.hover(&input, cancel)?;
                ProtocolOutput::for_request::<HoverRequest>(result, input.limit)
            },
            HoverFinished,
        )
    }
}

impl Handler<HoverFinished> for SessionActor {
    async fn handle(
        &mut self,
        _: &mut HandlerContext<'_, Self>,
        message: HoverFinished,
    ) -> io::Result<()> {
        let serial = message.0.serial;
        if let Some(result) = self.accept_request_work(message.0)? {
            self.respond_request(serial, result)?;
        }
        Ok(())
    }
}

pub(crate) fn capabilities(capabilities: &mut ServerCapabilities) {
    capabilities.hover_provider = Some(HoverProviderCapability::Simple(true));
}

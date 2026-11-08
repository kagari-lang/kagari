//! Implementation search owns its progress, context-sized steps and continuation.
use crate::{
    analysis::{
        AnalysisFailure,
        features::{
            DocumentQueryInput, ProtocolOutput, navigation::implementation::ImplementationStep,
        },
    },
    protocol::{ErrorCode, output::MESSAGE_BASE_CHARGE_BYTES, typed::TypedRequest},
    session::{SessionActor, request_tasks::WorkCompletion},
};
use lattice_actor::{
    context::{ActorContext, HandlerContext},
    traits::Handler,
};
use lsp_types::{
    ImplementationProviderCapability, ProgressToken, ServerCapabilities,
    StaticTextDocumentRegistrationOptions, request::GotoImplementation,
};
use serde_json::Value;
use std::io;

#[derive(lattice_actor::Message)]
pub(crate) struct ImplementationRequested(pub TypedRequest<GotoImplementation>);

enum StepOutput {
    Complete(ProtocolOutput),
    Continue,
}

#[derive(lattice_actor::Message)]
struct ImplementationFinished {
    work: WorkCompletion<Result<StepOutput, AnalysisFailure>>,
    input: DocumentQueryInput,
    contexts_indexed: usize,
}

impl Handler<ImplementationRequested> for SessionActor {
    async fn handle(
        &mut self,
        ctx: &mut HandlerContext<'_, Self>,
        message: ImplementationRequested,
    ) -> io::Result<()> {
        let Some(request) = self.admit_read(ctx, message.0)? else {
            return Ok(());
        };
        let progress = if self.progress.supported {
            request
                .params
                .work_done_progress_params
                .work_done_token
                .clone()
        } else {
            None
        };
        if matches!(&progress, Some(ProgressToken::String(token)) if token.len() > 256) {
            return self.reject(
                request.serial,
                ErrorCode::InvalidParams,
                "oversized work-done token",
            );
        }
        let Some(input) = self.prepare_document(
            request.serial,
            &request.params.text_document_position_params,
        )?
        else {
            return Ok(());
        };
        if let Some(token) = progress {
            let notification =
                match self
                    .progress
                    .begin(request.serial, token, "Find implementations")
                {
                    Ok(notification) => notification,
                    Err(error) => {
                        return self.reject(
                            request.serial,
                            ErrorCode::InvalidParams,
                            &error.to_string(),
                        );
                    }
                };
            self.outbox
                .push(notification.into(), None, MESSAGE_BASE_CHARGE_BYTES)?;
        }
        self.wait_for_document(input, |actor, ctx, input| {
            actor.start_implementation(ctx, input, 0)
        });
        Ok(())
    }
}

impl SessionActor {
    fn start_implementation(
        &mut self,
        ctx: &mut ActorContext<Self>,
        input: DocumentQueryInput,
        contexts_indexed: usize,
    ) -> io::Result<()> {
        let resume = input.clone();
        #[cfg(test)]
        let step_gate = self.operations.step_gate.clone();
        self.submit_request_work(
            ctx,
            input.serial,
            input.capture.clone(),
            move |state, cancel, _job| {
                let step = state.implementation(&input, cancel)?;
                match step {
                    ImplementationStep::Complete(value) => {
                        ProtocolOutput::for_request::<GotoImplementation>(value, input.limit)
                            .map(StepOutput::Complete)
                    }
                    ImplementationStep::Continue => {
                        #[cfg(test)]
                        if let Some(gate) = step_gate {
                            assert_eq!(state.index_cursor_count(), 1);
                            gate.hold(_job, input.capture.generation);
                        }
                        Ok(StepOutput::Continue)
                    }
                }
            },
            move |work| ImplementationFinished {
                work,
                input: resume,
                contexts_indexed,
            },
        )
    }
}

impl Handler<ImplementationFinished> for SessionActor {
    async fn handle(
        &mut self,
        _: &mut HandlerContext<'_, Self>,
        message: ImplementationFinished,
    ) -> io::Result<()> {
        let serial = message.work.serial;
        if let Some(result) = self.accept_request_work(message.work)? {
            match result {
                Ok(StepOutput::Complete(output)) => self.respond_request(serial, Ok(output))?,
                Ok(StepOutput::Continue) => {
                    let contexts_indexed = message.contexts_indexed + 1;
                    self.progress.report(
                        serial,
                        format!("Indexed {contexts_indexed} project contexts"),
                    );
                    self.requeue_document(message.input, move |actor, ctx, input| {
                        actor.start_implementation(ctx, input, contexts_indexed)
                    });
                }
                Err(error) => self.respond_request(serial, Err(error))?,
            }
        }
        Ok(())
    }
}

pub(crate) fn capabilities(capabilities: &mut ServerCapabilities) {
    capabilities.implementation_provider = Some(ImplementationProviderCapability::Options(
        StaticTextDocumentRegistrationOptions {
            document_selector: None,
            id: None,
        },
    ));
}

pub(crate) fn patch_capabilities(result: &mut Value) {
    // lsp-types 0.97 omits WorkDoneProgressOptions on implementation registration.
    result["capabilities"]["implementationProvider"]["workDoneProgress"] = Value::Bool(true);
}

//! Typed completion ingress uses the shared request admission policy.
use crate::{
    analysis::features::completion::CompletionQuery, protocol::Request, session::SessionActor,
};
use lattice_actor::{context::HandlerContext, traits::Handler};
use std::io;

#[derive(lattice_actor::Message)]
pub(crate) struct CompletionRequested(pub Request);

impl Handler<CompletionRequested> for SessionActor {
    async fn handle(
        &mut self,
        ctx: &mut HandlerContext<'_, Self>,
        message: CompletionRequested,
    ) -> io::Result<()> {
        self.admit_feature::<CompletionQuery>(ctx, message.0)
    }
}

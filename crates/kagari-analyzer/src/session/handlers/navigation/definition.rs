//! Typed definition ingress uses the shared request admission policy.
use crate::{
    analysis::features::navigation::definition::DefinitionQuery, protocol::Request,
    session::SessionActor,
};
use lattice_actor::{context::HandlerContext, traits::Handler};
use std::io;

#[derive(lattice_actor::Message)]
pub(crate) struct DefinitionRequested(pub Request);

impl Handler<DefinitionRequested> for SessionActor {
    async fn handle(
        &mut self,
        ctx: &mut HandlerContext<'_, Self>,
        message: DefinitionRequested,
    ) -> io::Result<()> {
        self.admit_feature::<DefinitionQuery>(ctx, message.0)
    }
}

//! Typed implementation ingress uses the shared request admission policy.
use crate::{
    analysis::features::navigation::implementation::ImplementationQuery, protocol::Request,
    session::SessionActor,
};
use lattice_actor::{context::HandlerContext, traits::Handler};
use std::io;

#[derive(lattice_actor::Message)]
pub(crate) struct ImplementationRequested(pub Request);

impl Handler<ImplementationRequested> for SessionActor {
    async fn handle(
        &mut self,
        ctx: &mut HandlerContext<'_, Self>,
        message: ImplementationRequested,
    ) -> io::Result<()> {
        self.admit_feature::<ImplementationQuery>(ctx, message.0)
    }
}

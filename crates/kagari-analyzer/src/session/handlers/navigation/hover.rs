//! Typed hover ingress uses the shared request admission policy.
use crate::{
    analysis::features::navigation::hover::HoverQuery, protocol::Request, session::SessionActor,
};
use lattice_actor::{context::HandlerContext, traits::Handler};
use std::io;

#[derive(lattice_actor::Message)]
pub(crate) struct HoverRequested(pub Request);

impl Handler<HoverRequested> for SessionActor {
    async fn handle(
        &mut self,
        ctx: &mut HandlerContext<'_, Self>,
        message: HoverRequested,
    ) -> io::Result<()> {
        self.admit_feature::<HoverQuery>(ctx, message.0)
    }
}

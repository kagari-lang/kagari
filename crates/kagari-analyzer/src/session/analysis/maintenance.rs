//! Coalesced idle cleanup retires cancelled index cursors without new client work.
use crate::session::SessionActor;
use kagari_common::cancellation::CancellationToken;
use lattice_actor::{
    context::{ActorContext, HandlerContext},
    traits::Handler,
};
use std::io::{self, Error};
use tokio::sync::{oneshot, oneshot::error::RecvError};

#[cfg(test)]
type MaintenanceOutput = usize;

#[cfg(not(test))]
type MaintenanceOutput = ();

#[derive(lattice_actor::Message)]
struct MaintenanceFinished {
    job: u64,
    result: Result<MaintenanceOutput, RecvError>,
}

impl SessionActor {
    pub(super) fn schedule_maintenance(
        &mut self,
        ctx: &mut ActorContext<Self>,
    ) -> io::Result<bool> {
        if !self.analysis.maintenance {
            return Ok(false);
        }
        let token = CancellationToken::default();
        let job = self.analysis.credit.reserve(token.clone())?;
        let live_serials = self.requests.unresolved().into_iter().collect();
        let input_generation = self.captures.generation;
        let mut retained = self.operations.retained_contexts();
        retained.extend(self.analysis.live_contexts.iter().cloned());
        let (sender, receiver) = oneshot::channel();
        ctx.pipe_to_self(receiver, move |result| MaintenanceFinished { job, result })
            .map_err(Error::other)?;
        self.analysis.maintenance = false;
        self.analysis
            .port
            .as_ref()
            .ok_or_else(|| Error::other("analysis worker unavailable"))?
            .submit(
                move |state, _| {
                    state.retain_contexts(&retained);
                    state.retain_requests(&live_serials);
                    state.retain_input_generation(input_generation);
                    #[cfg(test)]
                    {
                        state.index_cursor_count()
                    }
                },
                token,
                sender,
            )
            .map_err(Error::other)?;
        Ok(true)
    }
}

impl Handler<MaintenanceFinished> for SessionActor {
    async fn handle(
        &mut self,
        _: &mut HandlerContext<'_, Self>,
        message: MaintenanceFinished,
    ) -> io::Result<()> {
        self.analysis.credit.complete(message.job)?;
        let result = message
            .result
            .map_err(|_| Error::other("analysis worker lost maintenance completion"));
        #[cfg(test)]
        self.analysis
            .maintained
            .send_replace((message.job, result?));
        #[cfg(not(test))]
        result?;
        Ok(())
    }
}

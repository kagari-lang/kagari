//! Arbitration of the shared worker, independent of each producer's work flow.
use crate::session::{Lifecycle, SessionActor, analysis::Environment};
use lattice_actor::context::ActorContext;
use std::io;

impl SessionActor {
    pub fn schedule_analysis(&mut self, ctx: &mut ActorContext<Self>) -> io::Result<()> {
        if self.state != Lifecycle::Running || !self.analysis.credit.is_idle() {
            return Ok(());
        }
        if self.schedule_environment_analysis(ctx)? {
            return Ok(());
        }
        if !matches!(
            self.analysis.environment,
            Environment::Ready | Environment::Failed
        ) {
            return Ok(());
        }
        if self.schedule_request_work(ctx)? {
            return Ok(());
        }
        if self.schedule_maintenance(ctx)? {
            return Ok(());
        }
        self.schedule_diagnostics(ctx)
    }
}

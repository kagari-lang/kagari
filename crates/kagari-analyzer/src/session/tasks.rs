//! Fixed IO completion messages; failures remain visible even without more input.
use crate::{
    protocol::{DecodeError, ErrorCode, RequestId, Response},
    session::{Lifecycle, SessionActor, timers::Deadline},
    transport::{self, ReadyEndpoint},
};
use lattice_actor::{context::HandlerContext, traits::Handler};
use std::{
    io::{self, Error},
    time::Instant,
};

#[derive(lattice_actor::Message)]
pub(crate) struct TransportReady(pub io::Result<ReadyEndpoint>);

#[derive(lattice_actor::Message)]
pub(crate) struct ProtocolFailure(pub DecodeError);

#[derive(lattice_actor::Message)]
pub(crate) struct DriverExited {
    pub name: &'static str,
    pub result: io::Result<()>,
}

#[derive(lattice_actor::Message)]
pub(crate) struct FrameWritten(pub u64);

#[derive(lattice_actor::Message)]
pub(crate) struct EncodeRejected {
    pub sequence: u64,
    pub id: Option<RequestId>,
    pub reason: String,
}

impl Handler<TransportReady> for SessionActor {
    async fn handle(
        &mut self,
        ctx: &mut HandlerContext<'_, Self>,
        message: TransportReady,
    ) -> io::Result<()> {
        match message.0 {
            Ok(endpoint) => {
                transport::start(
                    ctx,
                    endpoint,
                    self.output.take().expect("single transport"),
                    self.budget.clone(),
                    self.stop.subscribe(),
                    &self.threads,
                )?;
                self.writer_ready = true;
            }
            Err(error) => self.fail(ctx, format!("transport startup: {error}")),
        }
        Ok(())
    }
}

impl Handler<ProtocolFailure> for SessionActor {
    async fn handle(
        &mut self,
        ctx: &mut HandlerContext<'_, Self>,
        message: ProtocolFailure,
    ) -> io::Result<()> {
        if message.0.fatal {
            self.fail(ctx, message.0.message);
            return Ok(());
        }
        self.logger.write(message.0.message);
        if let Some(response) = message.0.response {
            self.outbox.push(response.into(), None, 4096)?;
        }
        Ok(())
    }
}

impl Handler<DriverExited> for SessionActor {
    async fn handle(
        &mut self,
        ctx: &mut HandlerContext<'_, Self>,
        message: DriverExited,
    ) -> io::Result<()> {
        match message.result {
            Err(error) => self.fail(ctx, format!("{}: {error}", message.name)),
            Ok(()) if !self.exit_received && self.state != Lifecycle::Stopping => {
                self.fail(ctx, format!("unexpected {} termination", message.name))
            }
            Ok(()) => {}
        }
        Ok(())
    }
}

impl Handler<FrameWritten> for SessionActor {
    async fn handle(
        &mut self,
        _: &mut HandlerContext<'_, Self>,
        message: FrameWritten,
    ) -> io::Result<()> {
        let serial = self.outbox.acknowledge(message.0)?;
        self.timers.remove(Deadline::Write);
        if let Some(serial) = serial {
            self.requests.acknowledge(serial);
            if Some(serial) == self.shutdown_serial {
                self.shutdown_written = true;
                self.state = Lifecycle::AwaitExit;
                self.timers
                    .set(Deadline::Exit, Instant::now() + self.limits.exit);
            }
        }
        Ok(())
    }
}

impl Handler<EncodeRejected> for SessionActor {
    async fn handle(
        &mut self,
        _: &mut HandlerContext<'_, Self>,
        message: EncodeRejected,
    ) -> io::Result<()> {
        let publication = self.outbox.active_publication;
        let progress = self.outbox.active_progress;
        let serial = self.outbox.acknowledge(message.sequence)?;
        self.timers.remove(Deadline::Write);
        self.logger.write(message.reason);
        if let Some(serial) = progress {
            self.features.progress.disable_reports(serial);
            return Ok(());
        }
        if publication.is_some() {
            return Err(Error::other(
                "diagnostic publication exceeded its encoding budget",
            ));
        }
        if serial.is_none() {
            return Err(Error::other("control response cannot fit encoding budget"));
        }
        self.outbox.push(
            Response::new_err(
                message.id,
                ErrorCode::RequestFailed as i32,
                "result exceeds encoding budget".into(),
            )
            .into(),
            serial,
            4096,
        )
    }
}

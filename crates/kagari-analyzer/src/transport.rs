//! Fixed IO drivers over one codec and one typed protocol router.
use std::io::Error;
#[cfg(test)]
use tokio::io::{DuplexStream, split};
use tokio::time::timeout;

mod reader;
mod stdio;
mod tcp;
mod writer;

use crate::{
    logging::LogSink,
    server::{
        ThreadOwners,
        options::{Endpoint, SessionLimits},
    },
    session::{
        SessionActor,
        outbox::WriteFrame,
        tasks::{DriverExited, TransportReady},
    },
};
use lattice_actor::context::ActorContext;
use std::{io, sync::Arc};
use tokio::{
    net::TcpStream,
    sync::{OwnedSemaphorePermit, Semaphore, mpsc, oneshot, watch},
};

pub(crate) struct IngressPermit {
    _count: OwnedSemaphorePermit,
    _bytes: OwnedSemaphorePermit,
}

#[derive(Clone)]
pub(crate) struct IngressBudget {
    count: Arc<Semaphore>,
    bytes: Arc<Semaphore>,
}

impl IngressBudget {
    pub fn new(limits: &SessionLimits) -> Self {
        Self {
            count: Arc::new(Semaphore::new(limits.ingress_count)),
            bytes: Arc::new(Semaphore::new(limits.ingress_bytes)),
        }
    }

    pub fn close(&self) {
        self.count.close();
        self.bytes.close();
    }
}

pub(crate) enum ReadyEndpoint {
    Stdio,
    Tcp(TcpStream),
    #[cfg(test)]
    Memory(DuplexStream),
}

pub(crate) fn prepare(
    ctx: &mut ActorContext<SessionActor>,
    endpoint: Endpoint,
    logger: LogSink,
    limits: &SessionLimits,
) -> io::Result<()> {
    let startup_timeout = limits.startup;
    ctx.pipe_to_self(
        async move {
            timeout(startup_timeout, async move {
                match endpoint {
                    Endpoint::Stdio => Ok(ReadyEndpoint::Stdio),
                    Endpoint::Connect(address) => {
                        tcp::connect(address).await.map(ReadyEndpoint::Tcp)
                    }
                    Endpoint::Listen(address) => {
                        tcp::listen(address, logger).await.map(ReadyEndpoint::Tcp)
                    }
                    #[cfg(test)]
                    Endpoint::Memory(stream) => Ok(ReadyEndpoint::Memory(stream)),
                }
            })
            .await
            .map_err(Error::other)?
        },
        TransportReady,
    )
    .map_err(Error::other)?;
    Ok(())
}

pub(crate) struct Drivers {
    reader: DriverGuard,
    writer: DriverGuard,
}

pub(crate) struct DriverGuard {
    report: Option<oneshot::Sender<io::Result<()>>>,
}

impl DriverGuard {
    pub fn finish(mut self, result: io::Result<()>) {
        if let Some(report) = self.report.take() {
            let _ = report.send(result);
        }
    }
}

impl Drop for DriverGuard {
    fn drop(&mut self) {
        if let Some(report) = self.report.take() {
            let _ = report.send(Err(Error::other("driver terminated without a result")));
        }
    }
}

fn driver_guard(
    ctx: &mut ActorContext<SessionActor>,
    name: &'static str,
) -> io::Result<DriverGuard> {
    let (report, receiver) = oneshot::channel();
    ctx.pipe_to_self(
        async move {
            receiver
                .await
                .unwrap_or_else(|_| Err(Error::other("driver report disconnected")))
        },
        move |result| DriverExited { name, result },
    )
    .map_err(Error::other)?;
    Ok(DriverGuard {
        report: Some(report),
    })
}

pub(crate) fn start(
    ctx: &mut ActorContext<SessionActor>,
    endpoint: ReadyEndpoint,
    output: mpsc::Receiver<WriteFrame>,
    budget: IngressBudget,
    stop: watch::Receiver<bool>,
    threads: &ThreadOwners,
) -> io::Result<()> {
    let reader_guard = driver_guard(ctx, "reader")?;
    let writer_guard = driver_guard(ctx, "writer")?;
    let drivers = Drivers {
        reader: reader_guard,
        writer: writer_guard,
    };
    match endpoint {
        ReadyEndpoint::Stdio => stdio::start(ctx, output, budget, stop, threads, drivers),
        ReadyEndpoint::Tcp(stream) => {
            stream.set_nodelay(true)?;
            let (read, write) = stream.into_split();
            tcp::start(ctx, read, write, output, budget, stop, drivers);
            Ok(())
        }
        #[cfg(test)]
        ReadyEndpoint::Memory(stream) => {
            let (read, write) = split(stream);
            tcp::start(ctx, read, write, output, budget, stop, drivers);
            Ok(())
        }
    }
}

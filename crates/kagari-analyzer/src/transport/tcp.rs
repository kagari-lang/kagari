//! Async socket drivers scoped to the Lattice actor executor.
use crate::{
    logging::LogSink,
    session::{SessionActor, outbox::WriteFrame},
    transport::{Drivers, IngressBudget, reader::Reader, writer},
};
use lattice_actor::context::ActorContext;
use std::io::{Error, ErrorKind};
use std::{io, net::SocketAddr};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{mpsc, watch},
};

pub(crate) async fn connect(address: SocketAddr) -> io::Result<TcpStream> {
    TcpStream::connect(address).await
}

pub(crate) async fn listen(address: SocketAddr, logger: LogSink) -> io::Result<TcpStream> {
    let listener = TcpListener::bind(address).await?;
    logger.write(format!("listening on {}", listener.local_addr()?));
    let (stream, _) = listener.accept().await?;
    Ok(stream)
}

pub(crate) fn start<R, W>(
    ctx: &mut ActorContext<SessionActor>,
    mut read: R,
    mut write: W,
    mut output: mpsc::Receiver<WriteFrame>,
    budget: IngressBudget,
    stop: watch::Receiver<bool>,
    drivers: Drivers,
) where
    R: AsyncRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
{
    let mut reader = Reader::new(ctx.self_handle(), budget);
    let mut read_stop = stop.clone();
    ctx.spawn_scoped(async move {
        let result = async {
            let mut scratch = [0; 4096];
            loop {
                reader.reserve().await?;
                let read_count = read.read(&mut scratch).await?;
                if read_count == 0 {
                    reader.finish()?;
                    return Err(Error::new(
                        ErrorKind::UnexpectedEof,
                        "input closed without exit",
                    ));
                }
                if reader.push(&scratch[..read_count]).await? {
                    return Ok(());
                }
            }
        };
        let result =
            tokio::select! { result = result => result, _ = read_stop.changed() => Ok(()) };
        drivers.reader.finish(result);
    });
    let handle = ctx.self_handle();
    let mut write_stop = stop;
    ctx.spawn_scoped(async move {
        let result = async {
            while let Some(frame) = output.recv().await {
                let (sequence, bytes) = writer::encode(frame, &handle).await?;
                if let Some(bytes) = bytes {
                    write.write_all(&bytes).await?;
                    write.flush().await?;
                    writer::ack(&handle, sequence).await?;
                }
            }
            write.shutdown().await
        };
        let result =
            tokio::select! { result = result => result, _ = write_stop.changed() => Ok(()) };
        drivers.writer.finish(result);
    });
}

//! Blocking standard streams never occupy Tokio's blocking pool.
use crate::{
    server::ThreadOwners,
    session::{SessionActor, outbox::WriteFrame},
    transport::{Drivers, IngressBudget, reader::Reader, writer},
};
use lattice_actor::context::ActorContext;
use std::io::{Error, ErrorKind};
use std::{
    io::{self, Read, Write},
    thread::Builder,
};
use tokio::{
    runtime::Handle,
    sync::{mpsc, watch},
};

pub(crate) fn start(
    ctx: &mut ActorContext<SessionActor>,
    mut output: mpsc::Receiver<WriteFrame>,
    budget: IngressBudget,
    stop: watch::Receiver<bool>,
    threads: &ThreadOwners,
    drivers: Drivers,
) -> io::Result<()> {
    let runtime = Handle::current();
    let reader_runtime = runtime.clone();
    let mut reader = Reader::new(ctx.self_handle(), budget);
    let mut read_stop = stop.clone();
    let reader_thread = Builder::new().name("kagari-lsp-stdin".into()).spawn(move || {
        let result = (|| {
            let mut input = io::stdin().lock();
            let mut scratch = [0; 4096];
            loop {
                reader_runtime.block_on(reader.reserve())?;
                let count = input.read(&mut scratch)?;
                if count == 0 { reader.finish()?; return Err(Error::new(ErrorKind::UnexpectedEof, "input closed without exit")); }
                let exit = reader_runtime.block_on(async {
                    tokio::select! { result = reader.push(&scratch[..count]) => result, _ = read_stop.changed() => Ok(true) }
                })?;
                if exit { return Ok(()); }
            }
        })();
        drivers.reader.finish(result);
    })?;
    threads
        .lock()
        .map_err(|_| Error::other("thread owner poisoned"))?
        .push(reader_thread);
    let handle = ctx.self_handle();
    let mut write_stop = stop;
    let writer_thread = Builder::new().name("kagari-lsp-stdout".into()).spawn(move || {
        let result = (|| {
            let mut stdout = io::stdout().lock();
            loop {
                let frame = runtime.block_on(async { tokio::select! { frame = output.recv() => frame, _ = write_stop.changed() => None } });
                let Some(frame) = frame else { return Ok(()); };
                let (sequence, bytes) = runtime.block_on(writer::encode(frame, &handle))?;
                if let Some(bytes) = bytes {
                    stdout.write_all(&bytes)?;
                    stdout.flush()?;
                    runtime.block_on(writer::ack(&handle, sequence))?;
                }
            }
        })();
        drivers.writer.finish(result);
    })?;
    threads
        .lock()
        .map_err(|_| Error::other("thread owner poisoned"))?
        .push(writer_thread);
    Ok(())
}

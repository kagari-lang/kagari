//! Encoding completes before any frame bytes reach an endpoint.
use crate::{
    protocol::{self, Message, RequestId},
    session::{
        SessionActor,
        outbox::WriteFrame,
        tasks::{EncodeRejected, FrameWritten},
    },
};
use lattice_actor::handle::ActorHandle;
use std::io;
use std::io::Error;

pub(crate) async fn encode(
    frame: WriteFrame,
    handle: &ActorHandle<SessionActor>,
) -> io::Result<(u64, Option<Vec<u8>>)> {
    let sequence = frame.sequence;
    match protocol::encode(&frame.message, frame.charge) {
        Ok(bytes) => Ok((sequence, Some(bytes))),
        Err(error) => {
            let id: Option<RequestId> = match frame.message {
                Message::Response(response) => response.id,
                _ => None,
            };
            handle
                .tell(EncodeRejected {
                    sequence,
                    id,
                    reason: error.to_string(),
                })
                .await
                .map_err(|_| Error::other("session closed during encoding failure"))?;
            Ok((sequence, None))
        }
    }
}

pub(crate) async fn ack(handle: &ActorHandle<SessionActor>, sequence: u64) -> io::Result<()> {
    handle
        .tell(FrameWritten(sequence))
        .await
        .map_err(|_| Error::other("session closed during writer acknowledgement"))
}

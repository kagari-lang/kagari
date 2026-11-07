//! Incremental ingress with permits retained beyond Lattice mailbox prefetch.
use crate::{
    protocol::{
        self,
        codec::{FrameDecoder, FrameEvent},
        router::{Router, send},
    },
    session::{SessionActor, tasks::ProtocolFailure},
    transport::{IngressBudget, IngressPermit},
};
use lattice_actor::handle::ActorHandle;
use std::io;
use std::io::Error;
use tokio::sync::OwnedSemaphorePermit;

pub(crate) struct Reader {
    decoder: FrameDecoder,
    router: Router,
    handle: ActorHandle<SessionActor>,
    budget: IngressBudget,
    count: Option<OwnedSemaphorePermit>,
    bytes: Option<OwnedSemaphorePermit>,
}

impl Reader {
    pub fn new(handle: ActorHandle<SessionActor>, budget: IngressBudget) -> Self {
        Self {
            decoder: FrameDecoder::default(),
            router: Router::new(),
            handle,
            budget,
            count: None,
            bytes: None,
        }
    }

    pub async fn reserve(&mut self) -> io::Result<()> {
        if self.count.is_none() {
            self.count = Some(
                self.budget
                    .count
                    .clone()
                    .acquire_owned()
                    .await
                    .map_err(Error::other)?,
            );
        }
        Ok(())
    }

    pub async fn push(&mut self, mut bytes: &[u8]) -> io::Result<bool> {
        loop {
            self.reserve().await?;
            let progress = self.decoder.consume(bytes).map_err(Error::other)?;
            bytes = &bytes[progress.consumed..];
            match progress.event {
                FrameEvent::NeedMore => return Ok(false),
                FrameEvent::BodyLength(length) => {
                    self.bytes = Some(
                        self.budget
                            .bytes
                            .clone()
                            .acquire_many_owned(u32::try_from(length).map_err(Error::other)?)
                            .await
                            .map_err(Error::other)?,
                    );
                    self.decoder.grant_body().map_err(Error::other)?;
                }
                FrameEvent::Frame(body) => {
                    let permit = IngressPermit {
                        _count: self.count.take().expect("frame count reservation"),
                        _bytes: self.bytes.take().expect("frame body reservation"),
                    };
                    let decoded = protocol::decode(&body);
                    drop(body);
                    match decoded {
                        Ok(message) => {
                            if self
                                .router
                                .dispatch(self.handle.clone(), message, permit)
                                .await?
                            {
                                return Ok(true);
                            }
                        }
                        Err(error) => {
                            send(self.handle.clone(), ProtocolFailure(error), permit).await?
                        }
                    }
                    if bytes.is_empty() {
                        return Ok(false);
                    }
                }
            }
        }
    }

    pub fn finish(&mut self) -> io::Result<()> {
        self.decoder.finish().map_err(Error::other)
    }
}

//! One coalescing timer driver; stale queued notices are harmless.
use crate::session::SessionActor;
use lattice_actor::{
    context::{ActorContext, HandlerContext},
    traits::Handler,
};
use std::{collections::BTreeMap, io, time::Instant};
use tokio::{
    sync::{oneshot, watch},
    time::sleep_until,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Deadline {
    Initialize,
    Write,
    Exit,
    Cleanup,
    Diagnostics,
}

pub(crate) struct Timers {
    deadlines: BTreeMap<Deadline, Instant>,
    update: watch::Sender<Option<Instant>>,
}

impl Timers {
    pub fn new() -> (Self, watch::Receiver<Option<Instant>>) {
        let (update, receiver) = watch::channel(None);
        (
            Self {
                deadlines: BTreeMap::new(),
                update,
            },
            receiver,
        )
    }

    pub fn set(&mut self, key: Deadline, at: Instant) {
        self.deadlines.insert(key, at);
        self.publish();
    }

    pub fn remove(&mut self, key: Deadline) {
        self.deadlines.remove(&key);
        self.publish();
    }

    fn publish(&self) {
        self.update
            .send_replace(self.deadlines.values().copied().min());
    }

    fn take_due(&mut self) -> Vec<Deadline> {
        let now = Instant::now();
        let due: Vec<_> = self
            .deadlines
            .iter()
            .filter_map(|(key, at)| (*at <= now).then_some(*key))
            .collect();
        for key in &due {
            self.deadlines.remove(key);
        }
        self.publish();
        due
    }
}

#[derive(lattice_actor::Message)]
pub(crate) struct DeadlineElapsed(oneshot::Sender<()>);

pub(crate) fn start(
    ctx: &mut ActorContext<SessionActor>,
    mut receiver: watch::Receiver<Option<Instant>>,
) {
    let handle = ctx.self_handle();
    ctx.spawn_scoped(async move {
        loop {
            let next = *receiver.borrow_and_update();
            match next {
                None => {
                    if receiver.changed().await.is_err() {
                        break;
                    }
                }
                Some(at) => tokio::select! {
                    changed = receiver.changed() => if changed.is_err() { break; },
                    () = sleep_until(at.into()) => {
                        let (acknowledge, received) = oneshot::channel();
                        if handle.tell(DeadlineElapsed(acknowledge)).await.is_err() { break; }
                        // Only one outstanding notice, even while other deadlines change.
                        if received.await.is_err() { break; }
                    }
                },
            }
        }
    });
}

impl Handler<DeadlineElapsed> for SessionActor {
    async fn handle(
        &mut self,
        ctx: &mut HandlerContext<'_, Self>,
        message: DeadlineElapsed,
    ) -> io::Result<()> {
        for deadline in self.timers.take_due() {
            if deadline == Deadline::Diagnostics {
                self.diagnostics.due = true;
            } else {
                self.fail(ctx, format!("{deadline:?} deadline expired"));
            }
        }
        let _ = message.0.send(());
        Ok(())
    }
}

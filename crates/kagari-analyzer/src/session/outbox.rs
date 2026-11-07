//! Reliable output remains owned until a matching writer acknowledgement.
use crate::{protocol::Message, server::options::SessionLimits};
use std::{
    collections::VecDeque,
    io::{self, Error},
};

pub(crate) struct WriteFrame {
    pub sequence: u64,
    pub message: Message,
    pub serial: Option<u64>,
    pub charge: usize,
}

pub(crate) struct Outbox {
    queue: VecDeque<WriteFrame>,
    pub active: Option<(u64, Option<u64>, usize)>,
    pub active_publication: Option<u64>,
    bytes: usize,
    sequence: u64,
    limits: SessionLimits,
}

impl Outbox {
    pub fn new(limits: SessionLimits) -> Self {
        Self {
            queue: VecDeque::new(),
            active: None,
            active_publication: None,
            bytes: 0,
            sequence: 0,
            limits,
        }
    }

    pub fn push(&mut self, message: Message, serial: Option<u64>, charge: usize) -> io::Result<()> {
        if self.queue.len() + usize::from(self.active.is_some()) >= self.limits.outbox_count
            || charge > self.limits.outbox_bytes.saturating_sub(self.bytes)
        {
            return Err(Error::other("reliable output capacity exhausted"));
        }
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or_else(|| Error::other("write sequence exhausted"))?;
        self.bytes += charge;
        self.queue.push_back(WriteFrame {
            sequence: self.sequence,
            message,
            serial,
            charge,
        });
        Ok(())
    }

    pub fn grant(&mut self) -> Option<WriteFrame> {
        if self.active.is_some() {
            return None;
        }
        let frame = self.queue.pop_front()?;
        self.active = Some((frame.sequence, frame.serial, frame.charge));
        Some(frame)
    }

    /// A replaceable intent is materialized only at its final writer grant.
    /// The publication owner has already applied fairness and freshness checks.
    pub fn grant_publication(
        &mut self,
        message: Message,
        charge: usize,
        publication: u64,
    ) -> io::Result<WriteFrame> {
        if self.active.is_some()
            || self.queue.len() >= self.limits.outbox_count
            || charge > self.limits.outbox_bytes.saturating_sub(self.bytes)
        {
            return Err(Error::other("publication has no writer/output credit"));
        }
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or_else(|| Error::other("write sequence exhausted"))?;
        self.bytes += charge;
        self.active = Some((self.sequence, None, charge));
        self.active_publication = Some(publication);
        Ok(WriteFrame {
            sequence: self.sequence,
            message,
            serial: None,
            charge,
        })
    }

    pub fn has_queued(&self) -> bool {
        !self.queue.is_empty()
    }

    /// Keep an optional intent pending while reliable frames consume its bound.
    pub fn can_grant_publication(&self) -> bool {
        self.active.is_none()
            && self.queue.len() < self.limits.outbox_count
            && self.limits.result_bytes <= self.limits.outbox_bytes.saturating_sub(self.bytes)
    }

    pub fn acknowledge(&mut self, sequence: u64) -> io::Result<Option<u64>> {
        let Some((expected, serial, charge)) = self.active else {
            return Err(Error::other("writer acknowledgement without credit"));
        };
        if expected != sequence {
            return Err(Error::other("writer acknowledgement sequence mismatch"));
        }
        self.active = None;
        self.active_publication = None;
        self.bytes -= charge;
        Ok(serial)
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty() && self.active.is_none()
    }
}

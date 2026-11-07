//! Worker admission remains occupied until its exact typed completion arrives.
use kagari_common::cancellation::CancellationToken;
use std::io::{self, Error};

#[derive(Default)]
pub(crate) struct JobCredit {
    next: u64,
    active: Option<(u64, CancellationToken)>,
}

impl JobCredit {
    pub fn reserve(&mut self, cancel: CancellationToken) -> io::Result<u64> {
        if self.active.is_some() {
            return Err(Error::other("worker credit is already occupied"));
        }
        let id = self
            .next
            .checked_add(1)
            .ok_or_else(|| Error::other("worker job ID exhausted"))?;
        self.next = id;
        self.active = Some((id, cancel));
        Ok(id)
    }

    pub fn complete(&mut self, id: u64) -> io::Result<()> {
        if self.active.as_ref().map(|(active, _)| *active) != Some(id) {
            return Err(Error::other("worker completion does not own its credit"));
        }
        self.active = None;
        Ok(())
    }

    pub fn cancel(&self) {
        if let Some((_, token)) = &self.active {
            token.cancel();
        }
    }

    pub fn is_idle(&self) -> bool {
        self.active.is_none()
    }
}

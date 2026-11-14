//! Managed revision retirement invalidates non-owning metadata indexes afterward.
mod records;

use crate::module::ModuleKey;
use kagari_gc::error::GcError;
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};

/// The store owns this port; tickets contain no Runtime or managed references.
#[derive(Debug)]
pub(super) struct Retirements {
    port: Arc<Mutex<Port>>,
}

#[derive(Debug)]
struct Port {
    connected: bool,
    credits: usize,
    pending: VecDeque<ModuleKey>,
}

/// Exactly one ticket is moved into a revision. Its reserved release credit also
/// covers construction failure, so Drop cannot allocate or call a managed peer.
#[derive(Debug)]
pub(crate) struct RetirementTicket {
    port: Arc<Mutex<Port>>,
    key: ModuleKey,
}

struct RetirementBatch {
    port: Arc<Mutex<Port>>,
    keys: Vec<ModuleKey>,
    committed: bool,
}

impl Default for Retirements {
    fn default() -> Self {
        Self {
            port: Arc::new(Mutex::new(Port {
                connected: true,
                credits: 0,
                pending: VecDeque::new(),
            })),
        }
    }
}

impl Retirements {
    pub(super) fn reserve(&self, key: ModuleKey) -> Result<RetirementTicket, GcError> {
        let mut port = lock(&self.port);
        if !port.connected {
            return Err(GcError::Closed);
        }
        let credits = port
            .credits
            .checked_add(1)
            .ok_or(GcError::Limit("revision retirement credits"))?;
        let additional = credits.saturating_sub(port.pending.len());
        port.pending.try_reserve(additional)?;
        port.credits = credits;
        Ok(RetirementTicket {
            port: self.port.clone(),
            key,
        })
    }

    fn take(&self) -> Result<RetirementBatch, GcError> {
        let mut port = lock(&self.port);
        let mut keys = Vec::new();
        keys.try_reserve_exact(port.pending.len())?;
        keys.extend(port.pending.drain(..));
        Ok(RetirementBatch {
            port: self.port.clone(),
            keys,
            committed: false,
        })
    }
}

impl RetirementBatch {
    fn commit(mut self) {
        {
            let mut port = lock(&self.port);
            if port.connected {
                port.credits = port
                    .credits
                    .checked_sub(self.keys.len())
                    .expect("retirement release credits");
            }
        }
        self.committed = true;
    }
}

impl Drop for RetirementTicket {
    fn drop(&mut self) {
        let mut port = lock(&self.port);
        if port.connected {
            assert!(
                port.pending.len() < port.pending.capacity(),
                "reserved retirement release credit"
            );
            port.pending.push_back(self.key);
        }
    }
}

impl Drop for RetirementBatch {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        let mut port = lock(&self.port);
        if port.connected {
            for key in self.keys.drain(..) {
                assert!(
                    port.pending.len() < port.pending.capacity(),
                    "reserved retirement restore credit"
                );
                port.pending.push_back(key);
            }
        }
    }
}

impl Drop for Retirements {
    fn drop(&mut self) {
        let mut port = lock(&self.port);
        port.connected = false;
        port.pending.clear();
        port.credits = 0;
    }
}

fn lock(port: &Mutex<Port>) -> MutexGuard<'_, Port> {
    port.lock().unwrap_or_else(PoisonError::into_inner)
}

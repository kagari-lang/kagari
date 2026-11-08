//! Incoming request identity, cancellation and terminal response ownership.
use crate::protocol::RequestId;
use kagari_common::cancellation::CancellationToken;
use std::collections::{BTreeMap, HashMap};

pub(crate) type RequestSerial = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AdmissionError {
    Duplicate,
    Full,
    Exhausted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Admitted,
    Responding,
}

struct Entry {
    id: RequestId,
    method: String,
    token: CancellationToken,
    state: State,
    control: bool,
}

/// Counts both unresolved requests and committed responses awaiting writer acknowledgement.
/// A terminal decision releases wire-ID occupancy, never serial ownership or capacity.
pub(crate) struct RequestRegistry {
    capacity: usize,
    next: RequestSerial,
    entries: BTreeMap<RequestSerial, Entry>,
    unresolved: HashMap<RequestId, RequestSerial>,
}

impl RequestRegistry {
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            capacity,
            next: 0,
            entries: BTreeMap::new(),
            unresolved: HashMap::new(),
        }
    }

    /// A duplicate remains an ambiguity even when all admission credits are occupied.
    pub(crate) fn admit(
        &mut self,
        id: RequestId,
        method: String,
    ) -> Result<RequestSerial, AdmissionError> {
        self.admit_kind(id, method, false)
    }

    /// Lifecycle requests use two reserved slots while ordinary responses are pending.
    /// Wire-ID uniqueness still covers both classes, including failed lifecycle calls.
    pub(crate) fn admit_control(
        &mut self,
        id: RequestId,
        method: String,
    ) -> Result<RequestSerial, AdmissionError> {
        self.admit_kind(id, method, true)
    }

    fn admit_kind(
        &mut self,
        id: RequestId,
        method: String,
        control: bool,
    ) -> Result<RequestSerial, AdmissionError> {
        if self.unresolved.contains_key(&id) {
            return Err(AdmissionError::Duplicate);
        }
        let capacity = if control { 2 } else { self.capacity };
        if self
            .entries
            .values()
            .filter(|entry| entry.control == control)
            .count()
            >= capacity
        {
            return Err(AdmissionError::Full);
        }
        let serial = self.next.checked_add(1).ok_or(AdmissionError::Exhausted)?;
        self.next = serial;
        self.unresolved.insert(id.clone(), serial);
        self.entries.insert(
            serial,
            Entry {
                id,
                method,
                token: CancellationToken::default(),
                state: State::Admitted,
                control,
            },
        );
        Ok(serial)
    }

    /// Commits the one terminal response. Call only after reserving its output slot.
    /// Repeated or late completions cannot alter a later request reusing this wire ID.
    pub(crate) fn terminal(&mut self, serial: RequestSerial) -> Option<RequestId> {
        let entry = self.entries.get_mut(&serial)?;
        if entry.state == State::Responding {
            return None;
        }
        entry.state = State::Responding;
        let removed = self.unresolved.remove(&entry.id);
        assert_eq!(removed, Some(serial), "request wire-ID ownership diverged");
        Some(entry.id.clone())
    }

    /// Releases a committed response's admission credit after its exact writer acknowledgement.
    pub(crate) fn acknowledge(&mut self, serial: RequestSerial) -> bool {
        if self
            .entries
            .get(&serial)
            .is_some_and(|entry| entry.state == State::Responding)
        {
            self.entries.remove(&serial);
            true
        } else {
            false
        }
    }

    /// Signals only the currently unresolved use of this wire ID.
    /// The caller still commits a cancellation response through `terminal`.
    pub(crate) fn cancel(&self, id: &RequestId) -> Option<RequestSerial> {
        let serial = *self.unresolved.get(id)?;
        self.entries.get(&serial)?.token.cancel();
        Some(serial)
    }

    /// Progress cancellation addresses an admitted serial instead of a wire ID.
    pub(crate) fn cancel_serial(&self, serial: RequestSerial) -> bool {
        let Some(entry) = self
            .entries
            .get(&serial)
            .filter(|entry| entry.state == State::Admitted)
        else {
            return false;
        };
        entry.token.cancel();
        true
    }

    pub(crate) fn token(&self, serial: RequestSerial) -> Option<CancellationToken> {
        self.entries.get(&serial).map(|entry| entry.token.clone())
    }

    pub(crate) fn method(&self, serial: RequestSerial) -> Option<&str> {
        self.entries.get(&serial).map(|entry| entry.method.as_str())
    }

    /// Stable serial order is useful when shutdown cancels every unresolved request.
    pub(crate) fn unresolved(&self) -> Vec<RequestSerial> {
        self.entries
            .iter()
            .filter_map(|(serial, entry)| (entry.state == State::Admitted).then_some(*serial))
            .collect()
    }

    #[cfg(any(test, feature = "verification"))]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_releases_wire_id_before_acknowledgement_without_reusing_serials() {
        let mut requests = RequestRegistry::new(2);
        let id = RequestId::from("same".to_owned());
        let old = requests.admit(id.clone(), "hover".into()).unwrap();
        let old_token = requests.token(old).unwrap();
        assert_eq!(requests.method(old), Some("hover"));
        assert_eq!(
            requests.admit(id.clone(), "completion".into()),
            Err(AdmissionError::Duplicate)
        );
        assert!(!requests.acknowledge(old));
        assert_eq!(requests.cancel(&id), Some(old));
        assert!(old_token.check().is_err());
        assert_eq!(requests.terminal(old), Some(id.clone()));
        assert_eq!(requests.cancel(&id), None);
        assert_eq!(requests.len(), 1);

        let new = requests.admit(id.clone(), "completion".into()).unwrap();
        assert_ne!(old, new);
        assert!(requests.token(new).unwrap().check().is_ok());
        assert_eq!(requests.unresolved(), [new]);
        assert_eq!(requests.terminal(old), None);
        assert_eq!(requests.method(new), Some("completion"));
        assert_eq!(
            requests.admit(RequestId::from(3), "hover".into()),
            Err(AdmissionError::Full)
        );
        assert!(requests.acknowledge(old));
        assert!(!requests.acknowledge(old));
        assert_eq!(requests.unresolved(), [new]);
        assert_eq!(requests.terminal(new), Some(id.clone()));
        assert_eq!(requests.cancel(&id), None);
        assert!(requests.token(new).unwrap().check().is_ok());
        assert!(requests.acknowledge(new));
        assert_eq!(requests.len(), 0);
        assert!(requests.unresolved().is_empty());
        assert_eq!(requests.token(old), None);
        assert_eq!(requests.terminal(old), None);
        assert_eq!(requests.terminal(new), None);
    }

    #[test]
    fn failed_admission_preserves_existing_ownership_and_checked_serial_exhaustion() {
        let mut requests = RequestRegistry::new(1);
        let first = requests.admit(RequestId::from(1), "first".into()).unwrap();
        assert_eq!(
            requests.admit(RequestId::from(1), "duplicate".into()),
            Err(AdmissionError::Duplicate)
        );
        assert_eq!(
            requests.admit(RequestId::from("1".to_owned()), "distinct".into()),
            Err(AdmissionError::Full)
        );
        assert_eq!(requests.method(first), Some("first"));
        assert_eq!(requests.cancel(&RequestId::from("1".to_owned())), None);
        assert!(requests.token(first).unwrap().check().is_ok());
        assert_eq!(requests.terminal(first), Some(RequestId::from(1)));
        assert!(requests.acknowledge(first));
        requests.next = u64::MAX - 1;
        let last = requests.admit(RequestId::from(2), "last".into()).unwrap();
        assert_eq!(last, u64::MAX);
        assert_eq!(requests.terminal(last), Some(RequestId::from(2)));
        assert!(requests.acknowledge(last));
        assert_eq!(
            requests.admit(RequestId::from(3), "overflow".into()),
            Err(AdmissionError::Exhausted)
        );
        assert_eq!(requests.len(), 0);
        assert!(requests.unresolved().is_empty());
    }

    #[test]
    fn lifecycle_slots_remain_serviceable_when_ordinary_responses_are_pending() {
        let mut requests = RequestRegistry::new(1);
        let query = requests.admit(RequestId::from(1), "hover".into()).unwrap();
        requests.terminal(query).unwrap();
        assert_eq!(
            requests.admit(RequestId::from(2), "hover".into()),
            Err(AdmissionError::Full)
        );
        let shutdown = requests
            .admit_control(RequestId::from(2), "shutdown".into())
            .unwrap();
        assert_eq!(
            requests.admit(RequestId::from(2), "hover".into()),
            Err(AdmissionError::Duplicate)
        );
        let rejected_initialize = requests
            .admit_control(RequestId::from(3), "initialize".into())
            .unwrap();
        requests.terminal(rejected_initialize).unwrap();
        assert_eq!(
            requests.admit_control(RequestId::from(4), "initialize".into()),
            Err(AdmissionError::Full)
        );
        assert_eq!(requests.unresolved(), [shutdown]);
        assert!(requests.acknowledge(query));
        let next_query = requests.admit(RequestId::from(4), "hover".into()).unwrap();
        assert_eq!(requests.len(), 3);
        assert_eq!(requests.terminal(shutdown), Some(RequestId::from(2)));
        assert!(requests.acknowledge(shutdown));
        assert!(requests.acknowledge(rejected_initialize));
        requests.terminal(next_query).unwrap();
        assert!(requests.acknowledge(next_query));
        assert_eq!(requests.len(), 0);
    }
}

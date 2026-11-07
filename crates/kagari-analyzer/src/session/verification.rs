//! Bounded FZ02 sequences over production request and output owners.
use crate::{
    protocol::{Message, Notification, RequestId, Response},
    server::options::SessionLimits,
    session::{
        outbox::Outbox,
        requests::{AdmissionError, Requests},
    },
};
use kagari_common::cancellation::CancellationToken;
use serde_json::{Value, json};

mod publications;

#[derive(Clone, Copy, PartialEq, Eq)]
struct ModelId(u8);

impl ModelId {
    fn wire(self) -> RequestId {
        if self.0 < 4 {
            RequestId::Integer(i32::from(self.0))
        } else {
            RequestId::String((self.0 - 4).to_string())
        }
    }
}

struct Admission {
    id: ModelId,
    serial: u64,
    method: String,
    terminal: bool,
    retired: bool,
    cancelled: bool,
    token: CancellationToken,
}

struct Publication {
    serial: Option<u64>,
    sequence: u64,
    charge: usize,
    message: Message,
    granted: bool,
    acknowledged: bool,
}

/// The model is an append-only history: occupancy and ordering are derived by
/// scanning facts, independently of production's wire map, serial table and FIFO.
struct Model {
    capacity: usize,
    limits: SessionLimits,
    admissions: Vec<Admission>,
    publications: Vec<Publication>,
}

impl Model {
    fn live(&self, id: ModelId) -> Option<usize> {
        self.admissions
            .iter()
            .position(|entry| entry.id == id && !entry.terminal)
    }

    fn occupancy(&self) -> usize {
        self.admissions
            .iter()
            .filter(|entry| !entry.retired)
            .count()
    }

    fn active(&self) -> Option<usize> {
        self.publications
            .iter()
            .position(|entry| entry.granted && !entry.acknowledged)
    }

    fn publish(
        &mut self,
        outbox: &mut Outbox,
        message: Message,
        serial: Option<u64>,
        charge: usize,
    ) -> bool {
        let count = self
            .publications
            .iter()
            .filter(|entry| !entry.acknowledged)
            .count();
        let bytes: usize = self
            .publications
            .iter()
            .filter(|entry| !entry.acknowledged)
            .map(|entry| entry.charge)
            .sum();
        let allowed =
            count < self.limits.outbox_count && bytes + charge <= self.limits.outbox_bytes;
        assert_eq!(
            outbox.push(message.clone(), serial, charge).is_ok(),
            allowed
        );
        if allowed {
            self.publications.push(Publication {
                serial,
                sequence: self.publications.len() as u64 + 1,
                charge,
                message,
                granted: false,
                acknowledged: false,
            });
        }
        allowed
    }

    fn grant(&mut self, outbox: &mut Outbox) {
        let expected = if self.active().is_none() {
            self.publications.iter().position(|entry| !entry.granted)
        } else {
            None
        };
        match (outbox.grant(), expected) {
            (None, None) => {}
            (Some(frame), Some(index)) => {
                let expected = &mut self.publications[index];
                assert_eq!(frame.sequence, expected.sequence);
                assert_eq!(frame.serial, expected.serial);
                assert_eq!(frame.charge, expected.charge);
                assert_eq!(frame.message, expected.message);
                expected.granted = true;
            }
            _ => panic!("output grant disagrees with outstanding publication history"),
        }
    }

    fn acknowledge(&mut self, requests: &mut Requests, outbox: &mut Outbox, valid: bool) {
        let active = self.active();
        let sequence = active.map_or(0, |index| self.publications[index].sequence);
        if active.is_none() || !valid {
            assert!(outbox.acknowledge(sequence + 1).is_err());
            return;
        }
        let entry = &mut self.publications[active.unwrap()];
        assert_eq!(outbox.acknowledge(sequence).unwrap(), entry.serial);
        entry.acknowledged = true;
        if let Some(serial) = entry.serial {
            let admission = self
                .admissions
                .iter_mut()
                .find(|entry| entry.serial == serial)
                .unwrap();
            assert!(admission.terminal && !admission.retired);
            assert!(requests.acknowledge(serial));
            admission.retired = true;
            assert!(!requests.acknowledge(serial));
        }
    }

    fn finish(&mut self, requests: &mut Requests, outbox: &mut Outbox, serial: u64, charge: usize) {
        let index = self
            .admissions
            .iter()
            .position(|entry| entry.serial == serial);
        let Some(index) = index.filter(|index| !self.admissions[*index].terminal) else {
            assert_eq!(requests.terminal(serial), None);
            return;
        };
        let id = self.admissions[index].id.wire();
        let message = Response::new_ok(id.clone(), json!({"serial": serial})).into();
        if self.publish(outbox, message, Some(serial), charge) {
            assert_eq!(requests.terminal(serial), Some(id));
            self.admissions[index].terminal = true;
        }
    }

    fn verify(&self, requests: &Requests, outbox: &Outbox) {
        assert_eq!(requests.len(), self.occupancy());
        assert_eq!(
            requests.unresolved(),
            self.admissions
                .iter()
                .filter(|entry| !entry.terminal)
                .map(|entry| entry.serial)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            outbox.is_empty(),
            self.publications.iter().all(|entry| entry.acknowledged)
        );
        for entry in &self.admissions {
            assert_eq!(entry.token.check().is_err(), entry.cancelled);
            assert_eq!(requests.token(entry.serial).is_some(), !entry.retired);
            assert_eq!(
                requests.method(entry.serial),
                (!entry.retired).then_some(entry.method.as_str())
            );
        }
    }
}

/// First byte chooses request capacity 1..8; remaining triples select operation,
/// wire/serial identity and value. At most 128 operations and 64 KiB are admitted.
/// Actions cover admission, cancellation, completion, output grant/acknowledgement,
/// control-output saturation and stale serial acknowledgement. Nothing is caught.
pub(crate) fn check(input: &[u8]) {
    if input.len() > 64 * 1024 {
        return;
    }
    publications::check(input);
    let capacity = usize::from(input.first().copied().unwrap_or(0) % 8) + 1;
    let outbox_count = capacity * 3 + 8;
    let limits = SessionLimits {
        requests: capacity,
        outbox_count,
        outbox_bytes: outbox_count * 4096 + 1024,
        result_bytes: 1024,
        ..SessionLimits::default()
    };
    limits.validate().unwrap();
    let mut requests = Requests::new(capacity);
    let mut outbox = Outbox::new(limits.clone());
    let mut model = Model {
        capacity,
        limits,
        admissions: Vec::new(),
        publications: Vec::new(),
    };
    for action in input
        .get(1..)
        .unwrap_or_default()
        .as_chunks::<3>()
        .0
        .iter()
        .take(128)
    {
        let id = ModelId(action[1] % 8);
        let serial = u64::from(action[1]) % (model.admissions.len() as u64 + 2);
        match action[0] % 7 {
            0 => {
                let method = if action[2] % 2 == 0 {
                    "hover"
                } else {
                    "completion"
                }
                .to_owned();
                let expected = if model.live(id).is_some() {
                    Err(AdmissionError::Duplicate)
                } else if model.occupancy() == model.capacity {
                    Err(AdmissionError::Full)
                } else {
                    Ok(model.admissions.len() as u64 + 1)
                };
                assert_eq!(requests.admit(id.wire(), method.clone()), expected);
                if let Ok(serial) = expected {
                    model.admissions.push(Admission {
                        id,
                        serial,
                        method,
                        terminal: false,
                        retired: false,
                        cancelled: false,
                        token: requests.token(serial).unwrap(),
                    });
                }
            }
            1 => {
                let expected = model.live(id);
                assert_eq!(
                    requests.cancel(&id.wire()),
                    expected.map(|index| model.admissions[index].serial)
                );
                if let Some(index) = expected {
                    model.admissions[index].cancelled = true;
                }
            }
            2 => model.finish(
                &mut requests,
                &mut outbox,
                serial,
                usize::from(action[2]) + 1,
            ),
            3 => model.grant(&mut outbox),
            4 => model.acknowledge(&mut requests, &mut outbox, action[2] % 2 == 0),
            5 => {
                let message = Notification::new("$/test/control".into(), Value::Null).into();
                model.publish(&mut outbox, message, None, usize::from(action[2]) * 1024);
            }
            6 => {
                // Unknown and uncommitted responses cannot release admission credits.
                if !model
                    .admissions
                    .iter()
                    .any(|entry| entry.serial == serial && entry.terminal && !entry.retired)
                {
                    assert!(!requests.acknowledge(serial));
                }
            }
            _ => unreachable!(),
        }
        model.verify(&requests, &outbox);
    }
    while !outbox.is_empty() {
        model.grant(&mut outbox);
        model.acknowledge(&mut requests, &mut outbox, true);
        model.verify(&requests, &outbox);
    }
    for serial in requests.unresolved() {
        model.finish(&mut requests, &mut outbox, serial, 1);
        model.grant(&mut outbox);
        model.acknowledge(&mut requests, &mut outbox, true);
    }
    model.verify(&requests, &outbox);
    assert_eq!(requests.len(), 0);
}

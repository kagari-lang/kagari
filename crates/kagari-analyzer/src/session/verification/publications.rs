//! Append-only job history and a tiny independent diagnostic-set model for FZ02.
use crate::{
    analysis::diagnostics::group::DiagnosticGroup,
    documents::DocumentInput,
    protocol::{Message, Notification},
    server::options::SessionLimits,
    session::{
        jobs::JobCredit,
        outbox::Outbox,
        publications::{ContributionOwner, Publications},
    },
    workspace::ContextKey,
};
use kagari_common::{cancellation::CancellationToken, identity::ModuleIdentity};
use lsp_types::{Diagnostic, Position, Range};
use serde_json::{Value, json};
use std::collections::VecDeque;

const URIS: [&str; 2] = ["untitled:one", "untitled:two"];

#[derive(Clone, Copy)]
struct Stamp {
    version: i32,
    incarnation: u64,
}

#[derive(Clone, Copy)]
struct Contribution {
    uri: usize,
    message: u8,
}

#[derive(Clone, Copy)]
struct Intent {
    serial: u64,
}

struct Job {
    id: u64,
    generation: u64,
    owner: usize,
    contribution: Contribution,
    token: CancellationToken,
    cancelled: bool,
    complete: bool,
}

struct Model {
    generation: u64,
    incarnation: u64,
    documents: [Option<Stamp>; 2],
    contributions: [Option<Contribution>; 2],
    visible: [bool; 2],
    pending: [Option<Intent>; 2],
    publication_serial: u64,
    write_sequence: u64,
    reliable: VecDeque<u64>,
    active: Option<u64>,
    burst: u8,
    jobs: Vec<Job>,
    stopped: bool,
    version_support: bool,
}

impl Model {
    fn live_job(&self) -> Option<usize> {
        self.jobs.iter().position(|job| !job.complete)
    }

    fn queue(&mut self, uri: usize) {
        if self.visible[uri]
            || self
                .contributions
                .iter()
                .flatten()
                .any(|contribution| contribution.uri == uri)
        {
            self.publication_serial += 1;
            self.pending[uri] = Some(Intent {
                serial: self.publication_serial,
            });
        } else {
            self.pending[uri] = None;
        }
    }

    fn replace(
        &mut self,
        generation: u64,
        owner: usize,
        contribution: Option<Contribution>,
    ) -> bool {
        if self.stopped || generation != self.generation {
            return false;
        }
        let old = self.contributions[owner];
        self.contributions[owner] = contribution;
        for uri in 0..2 {
            if old.is_some_and(|old| old.uri == uri)
                || contribution.is_some_and(|new| new.uri == uri)
            {
                self.queue(uri);
            }
        }
        true
    }

    fn invalidate(&mut self, publications: &mut Publications, selector: u8) {
        if self.stopped {
            return;
        }
        self.generation += 1;
        let uri = usize::from(selector % 2);
        if selector & 0x80 != 0 {
            self.documents[uri] = None;
        } else {
            let incarnation = self.documents[uri].map_or_else(
                || {
                    self.incarnation += 1;
                    self.incarnation
                },
                |stamp| stamp.incarnation,
            );
            self.documents[uri] = Some(Stamp {
                version: i32::from(selector as i8),
                incarnation,
            });
        }
        self.contributions = [None, None];
        self.pending = [None, None];
        for uri in 0..2 {
            if self.visible[uri] {
                self.queue(uri);
            }
        }
        let documents = self
            .documents
            .iter()
            .enumerate()
            .filter_map(|(index, stamp)| {
                stamp.map(|stamp| DocumentInput {
                    uri: URIS[index].parse().unwrap(),
                    source_name: URIS[index].into(),
                    version: stamp.version,
                    incarnation: stamp.incarnation,
                    source: None,
                    unavailable: true,
                })
            })
            .collect::<Vec<_>>();
        publications
            .invalidate(self.generation, &documents)
            .unwrap();
    }

    fn expected(&self, uri: usize) -> Value {
        let records: Vec<_> = self
            .contributions
            .iter()
            .enumerate()
            .filter_map(|(owner, value)| {
                value
                    .filter(|value| value.uri == uri)
                    .map(|value| (owner, value.message))
            })
            .collect();
        let differing = records.len() == 2 && records[0].1 != records[1].1;
        let mut diagnostics = Vec::new();
        for (index, (owner, message)) in records.iter().enumerate() {
            if index > 0 && records[0].1 == *message {
                continue;
            }
            let mut message = format!("diagnostic-{message}");
            if differing {
                message.push_str(&format!("\nContexts: root {owner}"));
            }
            diagnostics.push(json!({"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":1}},"message":message}));
        }
        let mut params = json!({"uri":URIS[uri],"diagnostics":diagnostics});
        if self.version_support
            && let Some(stamp) = self.documents[uri]
        {
            params["version"] = json!(stamp.version);
        }
        params
    }

    fn grant(&mut self, publications: &mut Publications, outbox: &mut Outbox) {
        if self.active.is_some() {
            assert!(outbox.grant().is_none());
            return;
        }
        let candidate = self
            .pending
            .iter()
            .enumerate()
            .filter_map(|(uri, pending)| pending.map(|intent| (uri, intent.serial)))
            .min_by_key(|(_, serial)| *serial);
        let eligible = self.reliable.len() < 12;
        assert_eq!(outbox.can_grant_publication(), eligible);
        let publish =
            candidate.filter(|_| eligible && (self.reliable.is_empty() || self.burst >= 8));
        let actual = if eligible {
            publications.next(!self.reliable.is_empty()).unwrap()
        } else {
            None
        };
        match (publish, actual) {
            (Some((uri, serial)), Some(publication)) => {
                assert_eq!(publication.publication_serial, serial);
                assert_eq!(
                    publication.notification.method,
                    "textDocument/publishDiagnostics"
                );
                assert_eq!(publication.notification.params, self.expected(uri));
                self.visible[uri] = self
                    .contributions
                    .iter()
                    .flatten()
                    .any(|contribution| contribution.uri == uri);
                self.pending[uri] = None;
                self.burst = 0;
                self.write_sequence += 1;
                let frame = outbox
                    .grant_publication(publication.notification.into(), publication.charge, serial)
                    .unwrap();
                assert_eq!(frame.sequence, self.write_sequence);
                assert_eq!(frame.serial, None);
                self.active = Some(frame.sequence);
            }
            (None, None) => {
                let expected = self.reliable.pop_front();
                let actual = outbox.grant();
                assert_eq!(actual.as_ref().map(|frame| frame.sequence), expected);
                if let Some(frame) = actual {
                    assert_eq!(
                        frame.message,
                        Message::Notification(Notification::new(
                            "$/test/reliable".into(),
                            Value::Null
                        ))
                    );
                    self.burst = (self.burst + 1).min(8);
                    publications.reliable_granted();
                    self.active = expected;
                }
            }
            _ => panic!("publication eligibility disagrees with independent output order"),
        }
    }

    fn verify(&self, credit: &JobCredit) {
        assert_eq!(credit.is_idle(), self.live_job().is_none());
        for job in &self.jobs {
            assert_eq!(job.token.check().is_err(), job.cancelled);
        }
    }
}

fn owner(index: usize) -> ContributionOwner {
    ContributionOwner::Context(ContextKey {
        workspace: "fuzz".into(),
        root: ModuleIdentity::single_file(index.to_string()),
        epoch: 1,
    })
}

fn group(contribution: Contribution) -> DiagnosticGroup {
    DiagnosticGroup::new(
        URIS[contribution.uri].parse().unwrap(),
        vec![Diagnostic::new_simple(
            Range::new(Position::new(0, 0), Position::new(0, 1)),
            format!("diagnostic-{}", contribution.message),
        )],
    )
    .unwrap()
}

fn replace(
    publications: &mut Publications,
    model: &mut Model,
    generation: u64,
    index: usize,
    contribution: Option<Contribution>,
) {
    let expected = model.replace(generation, index, contribution);
    assert_eq!(
        publications
            .replace(
                generation,
                owner(index),
                format!("root {index}"),
                contribution.map(group).into_iter().collect()
            )
            .unwrap(),
        expected
    );
}

pub(super) fn check(input: &[u8]) {
    let limits = SessionLimits {
        // This model owns one JobCredit and no RequestRegistry registry. Preserve the
        // twelve-frame saturation boundary while satisfying session validation.
        requests: 1,
        outbox_count: 12,
        outbox_bytes: 65536,
        result_bytes: 1024,
        publication_uris: 2,
        publication_diagnostics: 4,
        publication_bytes: 32768,
        ..SessionLimits::default()
    };
    limits.validate().unwrap();
    let mut publications = Publications::new(&limits);
    let mut outbox = Outbox::new(limits);
    let mut credit = JobCredit::default();
    let mut model = Model {
        generation: 0,
        incarnation: 0,
        documents: [None, None],
        contributions: [None, None],
        visible: [false, false],
        pending: [None, None],
        publication_serial: 0,
        write_sequence: 0,
        reliable: VecDeque::new(),
        active: None,
        burst: 0,
        jobs: Vec::new(),
        stopped: false,
        version_support: input.first().is_some_and(|byte| byte & 1 != 0),
    };
    publications.set_version_support(model.version_support);
    model.invalidate(&mut publications, 0);
    for action in input
        .get(1..)
        .unwrap_or_default()
        .as_chunks::<3>()
        .0
        .iter()
        .take(128)
    {
        let index = usize::from(action[1] % 2);
        let contribution = Contribution {
            uri: usize::from(action[2] % 2),
            message: action[2] % 4,
        };
        match action[0] % 14 {
            0 => {
                model.invalidate(&mut publications, action[2]);
                credit.cancel();
                if let Some(index) = model.live_job() {
                    model.jobs[index].cancelled = true;
                }
            }
            1 if !model.stopped => {
                let token = CancellationToken::default();
                if model.live_job().is_some() {
                    assert!(credit.reserve(token).is_err());
                } else {
                    let id = credit.reserve(token.clone()).unwrap();
                    assert_eq!(id, model.jobs.len() as u64 + 1);
                    model.jobs.push(Job {
                        id,
                        generation: model.generation,
                        owner: index,
                        contribution,
                        token,
                        cancelled: false,
                        complete: false,
                    });
                }
            }
            2 => {
                credit.cancel();
                if let Some(index) = model.live_job() {
                    model.jobs[index].cancelled = true;
                }
            }
            3 => {
                if let Some(index) = model.live_job() {
                    let job = &mut model.jobs[index];
                    credit.complete(job.id).unwrap();
                    job.complete = true;
                    let (generation, owner, contribution) =
                        (job.generation, job.owner, job.contribution);
                    replace(
                        &mut publications,
                        &mut model,
                        generation,
                        owner,
                        Some(contribution),
                    );
                } else {
                    assert!(credit.complete(0).is_err());
                }
            }
            4 => {
                let forged = model.live_job().map_or(0, |index| {
                    if action[2] % 2 == 0 {
                        model.jobs[index].id + 1
                    } else {
                        model.jobs[index].id - 1
                    }
                });
                assert!(credit.complete(forged).is_err());
            }
            5 => {
                let generation = model.generation;
                replace(
                    &mut publications,
                    &mut model,
                    generation,
                    index,
                    Some(contribution),
                );
            }
            6 if action[2] & 15 == 0 => {
                publications.shutdown();
                model.stopped = true;
                model.pending = [None, None];
                model.contributions = [None, None];
                model.visible = [false, false];
                credit.cancel();
                if let Some(index) = model.live_job() {
                    model.jobs[index].cancelled = true;
                }
            }
            7 if model.reliable.len() + usize::from(model.active.is_some()) < 12 => {
                outbox
                    .push(
                        Notification::new("$/test/reliable".into(), Value::Null).into(),
                        None,
                        64,
                    )
                    .unwrap();
                model.write_sequence += 1;
                model.reliable.push_back(model.write_sequence);
            }
            8 => model.grant(&mut publications, &mut outbox),
            9 => {
                if let Some(sequence) = model.active
                    && action[2] % 2 == 0
                {
                    assert_eq!(outbox.acknowledge(sequence).unwrap(), None);
                    model.active = None;
                } else {
                    assert!(outbox.acknowledge(model.active.unwrap_or(0) + 1).is_err());
                }
            }
            12 => {
                let generation = model.generation - 1;
                replace(
                    &mut publications,
                    &mut model,
                    generation,
                    index,
                    Some(contribution),
                );
            }
            13 => {
                let generation = model.generation;
                replace(&mut publications, &mut model, generation, index, None);
            }
            _ => {}
        }
        model.verify(&credit);
        assert!(publications.retained_bytes() <= 32768);
    }
    publications.shutdown();
    model.stopped = true;
    model.pending = [None, None];
    model.contributions = [None, None];
    model.visible = [false, false];
    credit.cancel();
    if let Some(index) = model.live_job() {
        model.jobs[index].cancelled = true;
        credit.complete(model.jobs[index].id).unwrap();
        model.jobs[index].complete = true;
    }
    while !outbox.is_empty() {
        if let Some(sequence) = model.active.take() {
            outbox.acknowledge(sequence).unwrap();
        } else {
            model.grant(&mut publications, &mut outbox);
        }
    }
    model.verify(&credit);
    assert_eq!(publications.retained_bytes(), 0);
    assert!(model.active.is_none() && model.reliable.is_empty());
}

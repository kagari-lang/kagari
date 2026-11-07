//! Generation-bound diagnostic contributions and replaceable URI publication intents.
use crate::{
    analysis::diagnostics::group::DiagnosticGroup,
    documents::DocumentInput,
    protocol::{Notification, source_name},
    server::options::SessionLimits,
    workspace::ContextKey,
};
use lsp_types::{PublishDiagnosticsParams, Uri};
use serde::Serialize;
use serde_json::{to_value, to_writer};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{self, Error, Write},
    mem::size_of,
    sync::Arc,
};

mod merge;

#[cfg(test)]
mod tests;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ContributionOwner {
    Context(ContextKey),
    Acquisition,
}

struct Contribution {
    label: String,
    groups: BTreeMap<String, DiagnosticGroup>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DocumentStamp {
    uri: Uri,
    incarnation: u64,
    version: i32,
}

#[derive(Clone)]
struct Intent {
    uri: Uri,
    generation: u64,
    document: Option<DocumentStamp>,
    serial: u64,
}

pub(crate) struct Publication {
    pub notification: Notification,
    pub charge: usize,
    pub publication_serial: u64,
}

pub(crate) struct Publications {
    limits: SessionLimits,
    generation: u64,
    serial: u64,
    documents: BTreeMap<String, DocumentStamp>,
    contributions: BTreeMap<ContributionOwner, Arc<Contribution>>,
    intents: BTreeMap<String, Intent>,
    /// Only last-granted nonempty sets need a future clear. Clear grants forget keys.
    visible: BTreeMap<String, Uri>,
    retained_bytes: usize,
    reliable_burst: u8,
    version_support: bool,
    stopped: bool,
}

impl Publications {
    pub fn new(limits: &SessionLimits) -> Self {
        Self {
            limits: limits.clone(),
            generation: 0,
            serial: 0,
            documents: BTreeMap::new(),
            contributions: BTreeMap::new(),
            intents: BTreeMap::new(),
            visible: BTreeMap::new(),
            retained_bytes: 0,
            reliable_burst: 0,
            version_support: false,
            stopped: false,
        }
    }

    pub fn set_version_support(&mut self, supported: bool) {
        self.version_support = supported;
    }

    /// Conservatively invalidates every context. Previously granted errors are
    /// cleared immediately; this pilot policy trades diagnostic flicker for freshness.
    pub fn invalidate(&mut self, generation: u64, inputs: &[DocumentInput]) -> io::Result<()> {
        if self.stopped {
            return Ok(());
        }
        if generation <= self.generation {
            return Err(Error::other("publication generation did not advance"));
        }
        let documents: BTreeMap<_, _> = inputs
            .iter()
            .map(|input| {
                (
                    input.source_name.clone(),
                    DocumentStamp {
                        uri: input.uri.clone(),
                        incarnation: input.incarnation,
                        version: input.version,
                    },
                )
            })
            .collect();
        if documents.len() != inputs.len() {
            return Err(Error::other("duplicate publication document identity"));
        }
        let contributions = BTreeMap::new();
        let mut intents = BTreeMap::new();
        let mut serial = self.serial;
        for (key, uri) in &self.visible {
            serial = next_serial(serial)?;
            intents.insert(
                key.clone(),
                Intent {
                    uri: uri.clone(),
                    generation,
                    document: stamp(&documents, uri)?,
                    serial,
                },
            );
        }
        let bytes = self.check_budget(&documents, &contributions, &intents, &self.visible)?;
        self.generation = generation;
        self.serial = serial;
        self.documents = documents;
        self.contributions = contributions;
        self.intents = intents;
        self.retained_bytes = bytes;
        Ok(())
    }

    /// Replaces one owner's complete contribution atomically. Old generations
    /// retire without changing current intents; budget errors leave state unchanged.
    pub fn replace(
        &mut self,
        generation: u64,
        owner: ContributionOwner,
        label: String,
        groups: Vec<DiagnosticGroup>,
    ) -> io::Result<bool> {
        if self.stopped || generation != self.generation {
            return Ok(false);
        }
        let mut contribution = Contribution {
            label,
            groups: BTreeMap::new(),
        };
        let mut empty_targets = BTreeMap::new();
        let mut supplied = BTreeSet::new();
        for group in groups {
            source_name(group.uri()).map_err(Error::other)?;
            if group.uri().as_str().len() > self.limits.document_uri_bytes {
                return Err(Error::other(
                    "diagnostic URI exceeds publication metadata limit",
                ));
            }
            let key = group.uri().as_str().to_owned();
            if !supplied.insert(key.clone()) {
                return Err(Error::other("duplicate URI in one diagnostic contribution"));
            }
            if group.len() == 0 {
                empty_targets.insert(key, group.uri().clone());
                continue;
            }
            if contribution
                .groups
                .insert(group.uri().as_str().to_owned(), group)
                .is_some()
            {
                return Err(Error::other("duplicate URI in one diagnostic contribution"));
            }
        }
        let mut affected: BTreeSet<String> = contribution.groups.keys().cloned().collect();
        affected.extend(empty_targets.keys().cloned());
        if let Some(old) = self.contributions.get(&owner) {
            affected.extend(old.groups.keys().cloned());
        }
        // Clone only metadata and Arc handles; immutable diagnostic buffers stay shared.
        let mut contributions = self.contributions.clone();
        if contribution.groups.is_empty() {
            contributions.remove(&owner);
        } else {
            contributions.insert(owner, Arc::new(contribution));
        }
        let mut intents = self.intents.clone();
        let mut serial = self.serial;
        for key in affected {
            let uri = contributions
                .values()
                .find_map(|contribution| contribution.groups.get(&key).map(|group| group.uri()))
                .or_else(|| self.visible.get(&key))
                .or_else(|| empty_targets.get(&key));
            if let Some(uri) = uri {
                serial = next_serial(serial)?;
                intents.insert(
                    key,
                    Intent {
                        uri: uri.clone(),
                        generation,
                        document: stamp(&self.documents, uri)?,
                        serial,
                    },
                );
            } else {
                intents.remove(&key);
            }
        }
        let bytes = self.check_budget(&self.documents, &contributions, &intents, &self.visible)?;
        self.serial = serial;
        self.contributions = contributions;
        self.intents = intents;
        self.retained_bytes = bytes;
        Ok(true)
    }

    /// Called only with idle writer credit. Eight reliable grants yield to one
    /// eligible diagnostic; without reliable output, diagnostics are ready immediately.
    pub fn next(&mut self, reliable_waiting: bool) -> io::Result<Option<Publication>> {
        if self.stopped || (reliable_waiting && self.reliable_burst < 8) {
            return Ok(None);
        }
        loop {
            let Some(key) = self
                .intents
                .iter()
                .min_by_key(|(_, intent)| intent.serial)
                .map(|(key, _)| key.clone())
            else {
                return Ok(None);
            };
            let intent = self.intents[&key].clone();
            if intent.generation != self.generation
                || intent.document != stamp(&self.documents, &intent.uri)?
            {
                self.intents.remove(&key);
                self.recount()?;
                continue;
            }
            let diagnostics =
                merge::diagnostics(&self.contributions, &key, self.limits.result_bytes)?;
            let nonempty = !diagnostics.is_empty();
            let params = PublishDiagnosticsParams {
                uri: intent.uri.clone(),
                diagnostics,
                version: self
                    .version_support
                    .then(|| intent.document.as_ref().map(|document| document.version))
                    .flatten(),
            };
            let mut counter = FrameCounter {
                bytes: 64,
                limit: self.limits.result_bytes,
            };
            to_writer(
                &mut counter,
                &WireNotification {
                    jsonrpc: "2.0",
                    method: "textDocument/publishDiagnostics",
                    params: &params,
                },
            )
            .map_err(Error::other)?;
            let notification = Notification::new(
                "textDocument/publishDiagnostics".into(),
                to_value(params).map_err(Error::other)?,
            );
            let mut visible = self.visible.clone();
            if nonempty {
                visible.insert(key.clone(), intent.uri);
            } else {
                visible.remove(&key);
            }
            let mut intents = self.intents.clone();
            intents.remove(&key);
            let bytes =
                self.check_budget(&self.documents, &self.contributions, &intents, &visible)?;
            self.visible = visible;
            self.intents = intents;
            self.retained_bytes = bytes;
            self.reliable_burst = 0;
            return Ok(Some(Publication {
                notification,
                charge: counter.bytes,
                publication_serial: intent.serial,
            }));
        }
    }

    pub fn reliable_granted(&mut self) {
        self.reliable_burst = self.reliable_burst.saturating_add(1).min(8);
    }

    pub fn shutdown(&mut self) {
        self.stopped = true;
        self.documents.clear();
        self.contributions.clear();
        self.intents.clear();
        self.visible.clear();
        self.retained_bytes = 0;
    }

    pub fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }

    fn recount(&mut self) -> io::Result<()> {
        self.retained_bytes = self.check_budget(
            &self.documents,
            &self.contributions,
            &self.intents,
            &self.visible,
        )?;
        Ok(())
    }

    fn check_budget(
        &self,
        documents: &BTreeMap<String, DocumentStamp>,
        contributions: &BTreeMap<ContributionOwner, Arc<Contribution>>,
        intents: &BTreeMap<String, Intent>,
        visible: &BTreeMap<String, Uri>,
    ) -> io::Result<usize> {
        let mut uris = BTreeSet::new();
        let mut bytes = 0usize;
        let mut diagnostics = 0usize;
        for (name, document) in documents {
            bytes = add(
                bytes,
                64 + size_of::<DocumentStamp>() + name.capacity() + document.uri.as_str().len(),
            )?;
        }
        for (owner, contribution) in contributions {
            bytes = add(
                bytes,
                owner_bytes(owner) + 64 + size_of::<Contribution>() + contribution.label.capacity(),
            )?;
            for (key, group) in &contribution.groups {
                uris.insert(key.as_str());
                diagnostics = add(diagnostics, group.len())?;
                bytes = add(bytes, 64 + key.capacity() + group.retained_bytes())?;
            }
        }
        for (key, intent) in intents {
            uris.insert(key.as_str());
            bytes = add(
                bytes,
                64 + size_of::<Intent>()
                    + key.capacity()
                    + intent.uri.as_str().len()
                    + intent
                        .document
                        .as_ref()
                        .map_or(0, |document| document.uri.as_str().len()),
            )?;
        }
        for (key, uri) in visible {
            uris.insert(key.as_str());
            bytes = add(
                bytes,
                64 + size_of::<Uri>() + key.capacity() + uri.as_str().len(),
            )?;
        }
        if uris.len() > self.limits.publication_uris
            || diagnostics > self.limits.publication_diagnostics
            || bytes > self.limits.publication_bytes
        {
            return Err(Error::other("diagnostic publication capacity exhausted"));
        }
        Ok(bytes)
    }
}

fn stamp(
    documents: &BTreeMap<String, DocumentStamp>,
    uri: &Uri,
) -> io::Result<Option<DocumentStamp>> {
    let name = source_name(uri).map_err(Error::other)?;
    Ok(documents
        .get(&name)
        .filter(|document| document.uri == *uri)
        .cloned())
}

fn next_serial(serial: u64) -> io::Result<u64> {
    serial
        .checked_add(1)
        .ok_or_else(|| Error::other("publication serial exhausted"))
}

fn add(left: usize, right: usize) -> io::Result<usize> {
    left.checked_add(right)
        .ok_or_else(|| Error::other("publication byte counter exhausted"))
}

fn owner_bytes(owner: &ContributionOwner) -> usize {
    size_of::<ContributionOwner>()
        + match owner {
            ContributionOwner::Acquisition => 0,
            ContributionOwner::Context(context) => {
                context.workspace.capacity()
                    + context.root.package.0.capacity()
                    + context.root.path.capacity() * size_of::<String>()
                    + context
                        .root
                        .path
                        .iter()
                        .map(String::capacity)
                        .sum::<usize>()
            }
        }
}

struct FrameCounter {
    bytes: usize,
    limit: usize,
}

#[derive(Serialize)]
struct WireNotification<'a> {
    jsonrpc: &'static str,
    method: &'static str,
    params: &'a PublishDiagnosticsParams,
}

impl Write for FrameCounter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes) {
            return Err(Error::other("diagnostic publication exceeds frame budget"));
        }
        self.bytes += bytes.len();
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

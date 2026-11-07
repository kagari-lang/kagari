//! Analysis scheduling owns jobs and publication eligibility, never HIR databases.
mod bootstrap;
mod diagnostics;
mod maintenance;
#[cfg(test)]
pub(crate) mod testing;

#[cfg(test)]
use crate::session::analysis::testing::CompletionGate;
use crate::{
    analysis::worker::AnalysisPort,
    documents::DocumentInput,
    session::{SessionActor, jobs::JobCredit, timers::Deadline},
    workspace::{ContextKey, WorkspaceCapture},
};
use kagari_source::generated::{GeneratedDocument, GeneratedDocuments};
use std::{
    collections::{BTreeSet, VecDeque},
    io,
    sync::Arc,
    time::{Duration, Instant},
};
#[cfg(test)]
use tokio::sync::watch;

pub(super) enum Environment {
    NeedStore,
    OpeningStore,
    NeedPreparation(GeneratedDocuments),
    Preparing,
    NeedPublication(Arc<[GeneratedDocument]>),
    Publishing,
    Ready,
    Failed,
}

#[derive(Clone)]
struct DiagnosticTask {
    capture: Arc<WorkspaceCapture>,
    context: Option<ContextKey>,
    documents: Arc<[DocumentInput]>,
}

pub(crate) struct AnalysisHost {
    pub port: Option<AnalysisPort>,
    pub credit: JobCredit,
    environment: Environment,
    epoch: u64,
    pending: VecDeque<DiagnosticTask>,
    pub(crate) background: bool,
    pub due: bool,
    pub(crate) maintenance: bool,
    pub(crate) live_contexts: BTreeSet<ContextKey>,
    #[cfg(test)]
    pub gate: Option<CompletionGate>,
    #[cfg(test)]
    pub completed: watch::Sender<(u64, u64)>,
    #[cfg(test)]
    pub maintained: watch::Sender<(u64, usize)>,
}

impl AnalysisHost {
    pub fn new() -> Self {
        Self {
            port: None,
            credit: JobCredit::default(),
            environment: Environment::NeedStore,
            epoch: 1,
            pending: VecDeque::new(),
            background: false,
            due: false,
            maintenance: false,
            live_contexts: BTreeSet::new(),
            #[cfg(test)]
            gate: None,
            #[cfg(test)]
            completed: watch::channel((0, 0)).0,
            #[cfg(test)]
            maintained: watch::channel((0, 0)).0,
        }
    }

    pub fn shutdown(&mut self) {
        self.credit.cancel();
        self.port = None;
        self.pending.clear();
        self.maintenance = false;
        self.live_contexts.clear();
        self.environment = Environment::Failed;
    }
}

impl SessionActor {
    pub fn invalidate_analysis(&mut self) -> io::Result<()> {
        self.analysis.pending.clear();
        self.analysis.due = false;
        self.analysis.maintenance = true;
        if self.analysis.background {
            self.analysis.credit.cancel();
        }
        self.publications
            .invalidate(self.captures.generation, &self.documents.inputs())?;
        self.timers.set(
            Deadline::Diagnostics,
            Instant::now() + Duration::from_millis(150),
        );
        Ok(())
    }

    pub fn accept_analysis_capture(&mut self, capture: Arc<WorkspaceCapture>) {
        self.analysis.live_contexts = capture
            .contexts
            .iter()
            .map(|context| context.key.clone())
            .collect();
        self.analysis.maintenance = true;
        let documents: Arc<[DocumentInput]> = self.documents.inputs().into();
        self.analysis.pending.clear();
        // Acquisition errors have their own contribution, including an empty workspace.
        self.analysis.pending.push_back(DiagnosticTask {
            capture: capture.clone(),
            context: None,
            documents: documents.clone(),
        });
        for context in &capture.contexts {
            self.analysis.pending.push_back(DiagnosticTask {
                capture: capture.clone(),
                context: Some(context.key.clone()),
                documents: documents.clone(),
            });
        }
    }
}

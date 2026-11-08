//! Analysis scheduling owns jobs and publication eligibility, never HIR databases.
mod bootstrap;
pub(crate) mod diagnostics;
mod maintenance;
mod scheduling;
#[cfg(test)]
pub(crate) mod testing;

#[cfg(test)]
use crate::session::analysis::testing::CompletionGate;
use crate::{
    analysis::worker::AnalysisPort,
    session::{SessionActor, jobs::JobCredit},
    workspace::{ContextKey, WorkspaceCapture},
};
use kagari_source::generated::{GeneratedDocument, GeneratedDocuments};
use std::{collections::BTreeSet, sync::Arc};
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

pub(crate) struct AnalysisHost {
    pub port: Option<AnalysisPort>,
    pub credit: JobCredit,
    environment: Environment,
    epoch: u64,
    pub(crate) preemptible: bool,
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
            preemptible: false,
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
        self.maintenance = false;
        self.live_contexts.clear();
        self.environment = Environment::Failed;
    }
}

impl SessionActor {
    pub fn accept_analysis_capture(&mut self, capture: Arc<WorkspaceCapture>) {
        self.analysis.live_contexts = capture
            .contexts
            .iter()
            .map(|context| context.key.clone())
            .collect();
        self.analysis.maintenance = true;
        self.accept_diagnostic_capture(capture);
    }
}

//! Immutable index inputs selected by the feature's search policy.
use crate::workspace::{ContextCapture, relations::ContextStamp};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::analysis) struct ScopeStamp {
    pub inputs: Vec<ContextStamp>,
}

pub(crate) struct IndexScope {
    pub contexts: Vec<ContextCapture>,
}

impl IndexScope {
    pub(in crate::analysis) fn stamp(&self) -> ScopeStamp {
        ScopeStamp {
            inputs: self
                .contexts
                .iter()
                .map(|context| ContextStamp {
                    context: context.key.clone(),
                    input: context.input_revision,
                })
                .collect(),
        }
    }
}

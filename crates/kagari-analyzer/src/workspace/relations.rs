//! Acquisition-owned physical package selections and discovery-failure attribution.
use crate::workspace::{CaptureIssue, ContextKey};
use kagari_common::identity::PackageId;
use kagari_package::identity::PackageSelection;
use std::collections::{BTreeMap, BTreeSet};

/// A portable package identity alone never establishes a shared provider.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ProviderKey {
    pub package: PackageId,
    pub physical: String,
    pub selection: PackageSelection,
}

/// Every selected user package in the root's dependency closure, even when its
/// source acquisition failed. Installed native declarations do not create edges.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ContextRelations {
    pub folders: BTreeSet<String>,
    pub providers: BTreeMap<PackageId, ProviderKey>,
}

/// Missing discovery whose dependency closure may not yet be known. Unknown
/// providers cannot be dismissed by comparing project directory spellings.
#[derive(Clone, Debug)]
pub(crate) struct DiscoveryIssue {
    pub issue: CaptureIssue,
    pub project: Option<String>,
    pub providers: Option<BTreeSet<ProviderKey>>,
}

impl DiscoveryIssue {
    pub fn unknown(issue: CaptureIssue) -> Self {
        Self {
            issue,
            project: None,
            providers: None,
        }
    }
}

/// Context epoch prevents reuse after retirement; the input revision covers the
/// complete source environment, physical origins and acquisition availability.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ContextStamp {
    pub context: ContextKey,
    pub input: u64,
}

//! Exact source diagnostics without executable leases, GC roots or private-call authority.
mod preparation;
mod query;

#[cfg(test)]
mod tests;

use crate::{
    closure::CallableObservation,
    host::HostRegistryId,
    module::{
        ExecutableModule,
        dispatch::{
            CallRoute, PublicationToken,
            identity::{PublicationDefinitions, StableDeclarationId},
        },
    },
};
use kagari_bytecode::source_package::VerifiedSourcePackage;
use kagari_common::{
    identity::{
        DefinitionPath, ModuleIdentity,
        table::{DefinitionId, DefinitionTable},
    },
    span::Span,
};
use kagari_contract::{
    source::{AuthoredKind, ExecutableSource},
    types::ConcreteFunctionIdentity,
};
use std::{collections::HashMap, sync::Arc};

/// An opaque authored locator. Read-only paths describe its original publication;
/// edits resolve its stable identity against the installation's selected package.
/// Neither paths nor offsets can be used to construct a locator.
#[derive(Debug, Clone)]
pub struct SourceDeclaration {
    info: Arc<DeclarationInfo>,
}

#[derive(Debug)]
struct DeclarationInfo {
    runtime: HostRegistryId,
    publication: PublicationToken,
    stable: StableDeclarationId,
    index: usize,
    ambiguous: bool,
    path: DefinitionPath,
    owner: Option<DefinitionPath>,
    kind: Option<AuthoredKind>,
}

impl SourceDeclaration {
    /// Exact authored kind when a source package was retained. Stripped products
    /// retain declaration paths but cannot distinguish every authored body form.
    pub fn kind(&self) -> Option<AuthoredKind> {
        self.info.kind
    }

    pub fn name(&self) -> &str {
        self.info
            .path
            .path
            .last()
            .map_or("", |part| part.name.as_str())
    }

    pub fn module(&self) -> &ModuleIdentity {
        &self.info.path.module
    }

    pub fn path(&self) -> &DefinitionPath {
        &self.info.path
    }

    pub fn owner(&self) -> Option<&DefinitionPath> {
        self.info.owner.as_ref()
    }

    pub fn publication(&self) -> PublicationToken {
        self.info.publication
    }
}

/// Selected source and association metadata only. Hosts explicitly own historical
/// snapshots; retaining one does not keep its publication executable.
#[derive(Debug, Clone)]
pub struct ProgramSourceSnapshot {
    data: Arc<SnapshotData>,
}

#[derive(Debug)]
struct SnapshotData {
    runtime: HostRegistryId,
    scope: Arc<PublicationDefinitions>,
    package: Option<VerifiedSourcePackage>,
    declarations: Vec<SourceDeclaration>,
    indices: HashMap<StableDeclarationId, Option<usize>>,
    changes: Vec<PublicationToken>,
    associations: HashMap<ConcreteFunctionIdentity, ExecutableSource>,
}

/// Opaque current route or copied exact observation. There is deliberately no
/// constructor accepting public numeric identities or borrowed execution storage.
#[derive(Debug, Clone)]
pub struct FunctionSourceTarget {
    selection: SourceSelection,
}

impl FunctionSourceTarget {
    /// Preserve a checked member identity without claiming an implementation body.
    pub(crate) fn unobserved_member(owner: &ExecutableModule, declaration: DefinitionPath) -> Self {
        let snapshot = owner.source_snapshot();
        let mut query = snapshot.unavailable(SourceUnavailableReason::NoObservedBody);
        if let SourceQuery::Unavailable(unavailable) = &mut query {
            unavailable.declaration = Some(declaration);
        }
        Self {
            selection: SourceSelection::Exact {
                runtime: snapshot.data.runtime,
                query: Arc::new(query),
            },
        }
    }
}

#[derive(Debug, Clone)]
enum SourceSelection {
    Current(CallRoute),
    Exact {
        runtime: HostRegistryId,
        query: Arc<SourceQuery>,
    },
}

/// Complete matching authored definition, not decompiled or current filesystem text.
#[derive(Debug, Clone)]
pub struct FunctionSource {
    pub declaration: SourceDeclaration,
    pub publication: PublicationToken,
    pub last_source_change: PublicationToken,
    pub uri: String,
    pub range: Span,
    pub text: String,
    pub fingerprint: u64,
    pub target: Option<CallableObservation>,
    pub executable: Option<ExecutableSource>,
    snapshot: ProgramSourceSnapshot,
}

impl FunctionSource {
    /// Scope/package for the exact selected root, including executable applications.
    pub fn snapshot(&self) -> &ProgramSourceSnapshot {
        &self.snapshot
    }
}

#[derive(Debug, Clone)]
pub enum SourceQuery {
    Available(FunctionSource),
    Unavailable(SourceUnavailable),
}

/// Known identity is preserved even when text is absent. Native bindings and
/// executable applications belong to `snapshot`'s definition scope when present.
#[derive(Debug, Clone)]
pub struct SourceUnavailable {
    /// Known declaration without implying a selected executable application.
    /// For an unbound interface member, this path includes its declaring trait.
    pub declaration: Option<DefinitionPath>,
    pub application: Option<ConcreteFunctionIdentity>,
    pub reason: SourceUnavailableReason,
    pub publication: Option<PublicationToken>,
    pub target: Option<CallableObservation>,
    pub executable: Option<ExecutableSource>,
    pub native_binding: Option<DefinitionId>,
    pub snapshot: Option<ProgramSourceSnapshot>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceUnavailableReason {
    NotRetained,
    MissingPackage,
    Mismatch,
    NoObservedBody,
    UnknownDeclaration,
    NativeOrigin,
}

impl ProgramSourceSnapshot {
    pub fn publication(&self) -> PublicationToken {
        self.data.scope.publication_token()
    }

    pub fn declarations(&self) -> &[SourceDeclaration] {
        &self.data.declarations
    }

    /// The exact checked package, independent of any optional compilation capsule.
    pub fn source_package(&self) -> Option<&VerifiedSourcePackage> {
        self.data.package.as_ref()
    }

    pub fn definitions(&self) -> &DefinitionTable {
        self.data.scope.definitions()
    }

    pub fn function(&self, declaration: &SourceDeclaration) -> SourceQuery {
        let index = match self.declaration_index(declaration) {
            Ok(index) => index,
            Err(reason) => return self.unavailable(reason),
        };
        let Some(package) = self.source_package() else {
            return self.unavailable(SourceUnavailableReason::MissingPackage);
        };
        let declaration = &self.data.declarations[index];
        let record = &package.records().declarations[index];
        if record.kind == AuthoredKind::Native {
            let mut result = self.unavailable(SourceUnavailableReason::NativeOrigin);
            if let SourceQuery::Unavailable(unavailable) = &mut result {
                unavailable.native_binding = self
                    .data
                    .associations
                    .values()
                    .find(|association| association.authored == Some(record.declaration))
                    .and_then(|association| association.native_binding);
            }
            return result;
        }
        let document = &package.records().documents[record.document as usize];
        SourceQuery::Available(FunctionSource {
            declaration: declaration.clone(),
            publication: self.publication(),
            last_source_change: self.data.changes[index],
            uri: document.uri.clone(),
            range: record.range,
            text: document.text[record.range.start..record.range.end].to_owned(),
            fingerprint: record.fingerprint,
            target: None,
            executable: None,
            snapshot: self.clone(),
        })
    }

    fn declaration_index(
        &self,
        declaration: &SourceDeclaration,
    ) -> Result<usize, SourceUnavailableReason> {
        if !self.same_installation(declaration) {
            return Err(SourceUnavailableReason::Mismatch);
        }
        if self.publication() == declaration.publication() {
            return Ok(declaration.info.index);
        }
        if declaration.info.ambiguous {
            return Err(SourceUnavailableReason::Mismatch);
        }
        match self.data.indices.get(&declaration.info.stable) {
            Some(Some(index)) => Ok(*index),
            Some(None) => Err(SourceUnavailableReason::Mismatch),
            None => Err(SourceUnavailableReason::UnknownDeclaration),
        }
    }

    fn same_installation(&self, declaration: &SourceDeclaration) -> bool {
        self.data.runtime == declaration.info.runtime
            && self.publication().installation_id() == declaration.publication().installation_id()
    }

    fn unavailable(&self, reason: SourceUnavailableReason) -> SourceQuery {
        SourceQuery::Unavailable(SourceUnavailable {
            declaration: None,
            application: None,
            reason,
            publication: Some(self.publication()),
            target: None,
            executable: None,
            native_binding: None,
            snapshot: Some(self.clone()),
        })
    }
}

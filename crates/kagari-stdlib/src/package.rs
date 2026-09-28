use crate::{
    index::{self, DeclarationSite},
    manifest::{BundledSource, bundled_sources},
};
use kagari_common::{
    Diagnostic, SourceFile, Span,
    cancellation::{CancellationToken, Cancelled},
    identity::{ModuleIdentity, PackageId},
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_syntax::{
    Parse,
    parser::{ParseLimits, parse_declarations},
};
use std::{collections::BTreeSet, sync::Arc};

#[derive(Debug, thiserror::Error)]
pub enum PackageError {
    #[error("standard package preparation cancelled")]
    Cancelled,
    #[error("invalid standard package manifest: {0}")]
    Manifest(String),
    #[error("invalid standard source {uri}")]
    Syntax {
        uri: String,
        diagnostics: Vec<Diagnostic>,
    },
    #[error("invalid native annotation in {uri}: {message}")]
    Annotation {
        uri: String,
        span: Span,
        message: String,
    },
}

impl From<Cancelled> for PackageError {
    fn from(_: Cancelled) -> Self {
        Self::Cancelled
    }
}

#[derive(Debug)]
pub struct ParsedStdlibFile {
    source: Arc<SourceFile>,
    parsed: Parse,
    declarations: Vec<DeclarationSite>,
}

impl ParsedStdlibFile {
    pub fn source(&self) -> &Arc<SourceFile> {
        &self.source
    }
    pub fn parsed(&self) -> &Parse {
        &self.parsed
    }
    pub fn declarations(&self) -> &[DeclarationSite] {
        &self.declarations
    }
}

/// A complete, immutable installed package. Retain it in the analysis owner and
/// share it across snapshots; cancellation/errors never publish a partial value.
#[derive(Debug)]
pub struct ParsedStdlibPackage {
    identity: PackageId,
    fingerprint: u64,
    files: Vec<ParsedStdlibFile>,
}

impl ParsedStdlibPackage {
    pub fn prepare(limits: ParseLimits, cancel: &CancellationToken) -> Result<Self, PackageError> {
        Self::prepare_sources(bundled_sources(), limits, cancel)
    }

    pub fn identity(&self) -> &PackageId {
        &self.identity
    }
    /// Deterministic content identity, not an authorization token or ABI seal.
    pub fn fingerprint(&self) -> u64 {
        self.fingerprint
    }
    pub fn files(&self) -> &[ParsedStdlibFile] {
        &self.files
    }

    pub(crate) fn prepare_sources(
        manifest: &[BundledSource],
        limits: ParseLimits,
        cancel: &CancellationToken,
    ) -> Result<Self, PackageError> {
        cancel.check()?;
        let identity = PackageId("kagari-std".into());
        let mut sources = SourceDatabase::new("/").map_err(PackageError::Manifest)?;
        let mut names = BTreeSet::new();
        let mut uris = BTreeSet::new();
        let mut files = Vec::with_capacity(manifest.len());
        let mut fingerprint = 0xcbf29ce484222325_u64;
        for entry in manifest {
            cancel.check()?;
            if !names.insert(entry.module) || !uris.insert(entry.uri) {
                return Err(PackageError::Manifest(
                    "duplicate module or source URI".into(),
                ));
            }
            let module = ModuleIdentity {
                package: identity.clone(),
                path: vec![entry.module.into()],
            };
            sources
                .bind_module(entry.uri, module)
                .map_err(PackageError::Manifest)?;
            let id = sources
                .set(entry.uri, entry.text.into(), SourceLayer::Base)
                .map_err(PackageError::Manifest)?;
            let source = sources
                .snapshot()
                .file(id)
                .expect("installed source")
                .clone();
            let parsed = parse_declarations(&source, limits, cancel)?;
            if !parsed.diagnostics().is_empty() {
                return Err(PackageError::Syntax {
                    uri: entry.uri.into(),
                    diagnostics: parsed.diagnostics().to_vec(),
                });
            }
            let declarations = index::declarations(&source, &parsed, cancel)?;
            for value in [entry.module, entry.uri, entry.text] {
                for byte in (value.len() as u64)
                    .to_le_bytes()
                    .into_iter()
                    .chain(value.bytes())
                {
                    fingerprint ^= u64::from(byte);
                    fingerprint = fingerprint.wrapping_mul(0x100000001b3);
                }
            }
            files.push(ParsedStdlibFile {
                source,
                parsed,
                declarations,
            });
        }
        cancel.check()?;
        Ok(Self {
            identity,
            fingerprint,
            files,
        })
    }
}

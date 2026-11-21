//! Owned registration/syntax inputs, independent of any lowered graph.
use super::{attach, validate_view};
use crate::{
    lower::LoweredModule,
    native::{
        paths::LibraryPaths,
        render::{DeclarationSource, render_with_paths},
    },
};
use kagari_common::{cancellation::CancellationToken, identity::metadata::DefinitionMetadata};
use kagari_source::{
    source::SourceFile,
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_syntax::parser::{Parse, ParseLimits, ParseOptions, parse};
use kagari_types::declaration::{
    module::{DeclarationError, ModuleDecl},
    ownership::ReceiverOwners,
};
use std::sync::{Arc, Weak};

type AttachedRegistration = (Option<Parse>, Arc<LoweredModule>);

/// Validated owned inputs. No lowering-local IDs or graph-bearing facts belong here.
#[derive(Debug, Clone)]
pub(crate) struct PreparedRegistration {
    pub(super) metadata: Arc<DefinitionMetadata<ModuleDecl>>,
    pub(super) providers: Arc<[Arc<DefinitionMetadata<ModuleDecl>>]>,
    pub(super) paths: Arc<LibraryPaths>,
    pub(super) source: Arc<SourceFile>,
    parsed: Option<Parse>,
}

impl PreparedRegistration {
    #[cfg(test)]
    pub(crate) fn new(
        metadata: Arc<DefinitionMetadata<ModuleDecl>>,
        providers: Arc<[Arc<DefinitionMetadata<ModuleDecl>>]>,
        supplied: Option<&DeclarationSource>,
        source_root: &str,
        limits: ParseLimits,
        cancel: &CancellationToken,
    ) -> Result<Self, DeclarationError> {
        let paths = Arc::new(LibraryPaths::new(&providers, &[]));
        Self::prepare(
            metadata,
            providers,
            paths,
            supplied,
            source_root,
            limits,
            cancel,
        )
    }

    /// Providers must already share the analysis scope produced by Native adoption.
    /// Supplied navigation text is checked against registration, never used as authority.
    fn prepare(
        metadata: Arc<DefinitionMetadata<ModuleDecl>>,
        providers: Arc<[Arc<DefinitionMetadata<ModuleDecl>>]>,
        paths: Arc<LibraryPaths>,
        supplied: Option<&DeclarationSource>,
        source_root: &str,
        limits: ParseLimits,
        cancel: &CancellationToken,
    ) -> Result<Self, DeclarationError> {
        check_cancel(cancel)?;
        let definition = metadata.records();
        let definitions = metadata.definitions();
        let owners = ReceiverOwners::from_types(
            providers
                .iter()
                .map(|module| module.records())
                .flat_map(|module| module.types.iter().map(|ty| (&module.identity, ty))),
        )?;
        definition.validate(&|receiver| owners.owner(receiver, definitions), definitions)?;
        // A source-less registration needs a logical module/file key for the existing
        // analysis graph, not declaration text. Its nodes carry Structured origins.
        let uri = supplied
            .map(|source| source.uri.clone())
            .unwrap_or_else(|| {
                format!(
                    "kagari-registration://{}/{}",
                    definition.identity.package.0,
                    definition.identity.path.join("/")
                )
            });
        let mut sources = SourceDatabase::new(source_root).map_err(DeclarationError)?;
        sources
            .bind_module(&uri, definition.identity.clone())
            .map_err(DeclarationError)?;
        let id = sources
            .set(
                &uri,
                supplied
                    .map(|source| source.text.clone())
                    .unwrap_or_default(),
                SourceLayer::Base,
            )
            .map_err(DeclarationError)?;
        let source = sources
            .snapshot()
            .file(id)
            .expect("native declaration source")
            .clone();
        let parsed = if supplied.is_some() {
            let parsed = parse(&source, ParseOptions { limits }, cancel)
                .map_err(|_| DeclarationError("native declaration analysis cancelled".into()))?;
            if !parsed.diagnostics().is_empty() {
                return Err(DeclarationError(format!(
                    "invalid native declaration syntax: {:?}",
                    parsed.diagnostics().first()
                )));
            }
            let expected = render_with_paths(&metadata, &providers, &paths)?;
            validate_view(&expected, &parsed, definition, cancel)?;
            Some(parsed)
        } else {
            None
        };
        check_cancel(cancel)?;
        Ok(Self {
            metadata,
            providers,
            paths,
            source,
            parsed,
        })
    }

    pub(crate) fn parsed(&self) -> Option<&Parse> {
        self.parsed.as_ref()
    }

    /// Real supplied source only; `source` otherwise exists solely as a graph key.
    pub(crate) fn source_surface(&self) -> Option<&Arc<SourceFile>> {
        self.parsed.as_ref().map(|_| &self.source)
    }

    /// Constructs fresh HIR directly, or lowers a supplied complete tool source.
    /// No lowering-local maps are reused across graph lifetimes.
    pub(crate) fn attach(
        &self,
        cancel: &CancellationToken,
    ) -> Result<Arc<LoweredModule>, DeclarationError> {
        check_cancel(cancel)?;
        let lowered = attach(self, cancel)?;
        check_cancel(cancel)?;
        Ok(Arc::new(lowered))
    }
}

/// Persistent owned inputs plus weak handles to live owned graphs.
/// Expired graphs are rebuilt from the retained inputs, never from old node-ID maps.
#[derive(Debug, Clone)]
pub(crate) struct NativeFiles {
    prepared: Vec<PreparedRegistration>,
    lowered: Vec<Weak<LoweredModule>>,
}

impl NativeFiles {
    pub(crate) fn new(
        modules: Vec<Arc<DefinitionMetadata<ModuleDecl>>>,
        paths: Arc<LibraryPaths>,
        sources: Option<&[DeclarationSource]>,
        source_root: &str,
        limits: ParseLimits,
        cancel: &CancellationToken,
    ) -> Result<Self, DeclarationError> {
        if sources.is_some_and(|sources| sources.len() != modules.len()) {
            return Err(DeclarationError(
                "native supplied source inventory differs from registration".into(),
            ));
        }
        let providers: Arc<[_]> = modules.into();
        let prepared = providers
            .iter()
            .enumerate()
            .map(|(index, module)| {
                PreparedRegistration::prepare(
                    module.clone(),
                    providers.clone(),
                    paths.clone(),
                    sources.map(|sources| &sources[index]),
                    source_root,
                    limits,
                    cancel,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        check_cancel(cancel)?;
        let lowered = (0..prepared.len()).map(|_| Weak::new()).collect();
        Ok(Self { prepared, lowered })
    }

    /// Publishes weak handles only after the entire attachment batch succeeds.
    pub(crate) fn attach(
        &mut self,
        cancel: &CancellationToken,
    ) -> Result<Vec<AttachedRegistration>, DeclarationError> {
        let mut attached = Vec::with_capacity(self.prepared.len());
        for (prepared, old) in self.prepared.iter().zip(&self.lowered) {
            check_cancel(cancel)?;
            let lowered = match old.upgrade() {
                Some(lowered) => lowered,
                None => prepared.attach(cancel)?,
            };
            attached.push((prepared.parsed.clone(), lowered));
        }
        check_cancel(cancel)?;
        self.lowered = attached
            .iter()
            .map(|(_, lowered)| Arc::downgrade(lowered))
            .collect();
        Ok(attached)
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.prepared.len()
    }
}

fn check_cancel(cancel: &CancellationToken) -> Result<(), DeclarationError> {
    cancel
        .check()
        .map_err(|_| DeclarationError("native declaration analysis cancelled".into()))
}

//! Owned registration/syntax inputs, independent of any lowered graph.
use super::{attach, validate_view};
use crate::{
    lower::LoweredModule,
    native::render::{DeclarationSource, declaration_source},
};
use kagari_common::{cancellation::CancellationToken, identity::metadata::DefinitionMetadata};
use kagari_source::{
    source::SourceFile,
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_syntax::parser::{Parse, ParseLimits, ParseMode, ParseOptions, parse};
use kagari_types::declaration::{
    module::{DeclarationError, ModuleDecl},
    ownership::ReceiverOwners,
};
use std::sync::{Arc, Weak};

/// Validated owned inputs. No lowering-local IDs or graph-bearing facts belong here.
#[derive(Debug, Clone)]
pub(crate) struct PreparedRegistration {
    pub(super) metadata: Arc<DefinitionMetadata<ModuleDecl>>,
    pub(super) providers: Arc<[Arc<DefinitionMetadata<ModuleDecl>>]>,
    pub(super) source: Arc<SourceFile>,
    parsed: Parse,
}

impl PreparedRegistration {
    /// Providers must already share the analysis scope produced by Native adoption.
    /// Supplied navigation text is checked against registration, never used as authority.
    pub(crate) fn new(
        metadata: Arc<DefinitionMetadata<ModuleDecl>>,
        providers: Arc<[Arc<DefinitionMetadata<ModuleDecl>>]>,
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
        let expected = declaration_source(&metadata, &providers)?;
        let generated = supplied.unwrap_or(&expected);
        let mut sources = SourceDatabase::new(source_root).map_err(DeclarationError)?;
        sources
            .bind_module(&generated.uri, definition.identity.clone())
            .map_err(DeclarationError)?;
        let id = sources
            .set(&generated.uri, generated.text.clone(), SourceLayer::Base)
            .map_err(DeclarationError)?;
        let source = sources
            .snapshot()
            .file(id)
            .expect("native declaration source")
            .clone();
        let parsed = parse(
            &source,
            ParseOptions {
                mode: ParseMode::Declarations,
                limits,
            },
            cancel,
        )
        .map_err(|_| DeclarationError("native declaration analysis cancelled".into()))?;
        if !parsed.diagnostics().is_empty() {
            return Err(DeclarationError(format!(
                "invalid native declaration syntax: {:?}",
                parsed.diagnostics().first()
            )));
        }
        if supplied.is_some() {
            validate_view(&expected, &parsed, limits, cancel)?;
        }
        check_cancel(cancel)?;
        Ok(Self {
            metadata,
            providers,
            source,
            parsed,
        })
    }

    pub(crate) fn parsed(&self) -> &Parse {
        &self.parsed
    }

    /// Lowers the exact cached parse and rebuilds all graph-local registration maps.
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

/// Persistent owned inputs plus transitional weak handles to live owned graphs.
/// At the borrowed-HIR cutover, these handles must move into analysis sessions.
#[derive(Debug, Clone)]
pub(crate) struct NativeFiles {
    prepared: Vec<PreparedRegistration>,
    lowered: Vec<Weak<LoweredModule>>,
}

impl NativeFiles {
    pub(crate) fn new(
        modules: Vec<Arc<DefinitionMetadata<ModuleDecl>>>,
        sources: Option<&[DeclarationSource]>,
        source_root: &str,
        limits: ParseLimits,
        cancel: &CancellationToken,
    ) -> Result<Self, DeclarationError> {
        let providers: Arc<[_]> = modules.into();
        let prepared = providers
            .iter()
            .enumerate()
            .map(|(index, module)| {
                PreparedRegistration::new(
                    module.clone(),
                    providers.clone(),
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
    ) -> Result<Vec<(Parse, Arc<LoweredModule>)>, DeclarationError> {
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

//! Explicit, source-backed library inputs for offline semantic analysis.
//!
//! Source owns declarations, fields, bounds and complete Script bodies. The small
//! sidecar carries constructor, Host schema/path, foundation/conversion and operation
//! facts that Kagari syntax does not express. Neither input grants installed bindings.

use crate::{
    analysis::{
        AnalysisDatabase,
        error::AnalysisError,
        inputs::LibrarySourceKey,
        source_preparation::{self, ParsedModule, SourceParseCache, SourcePreparer},
    },
    collections::HashSet,
    hir::item::function::{Function, FunctionImplementation, FunctionKind, NativeContract},
    lower::LoweredModule,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind, ModuleIdentity, map::DefinitionContext, mapping::DefinitionMappingError,
        metadata::DefinitionMetadata,
    },
};
use kagari_source::{identity::FileId, source::SourceFile, source_database::SourceSnapshot};
use kagari_types::declaration::names::NameNamespace;
use kagari_types::{
    callable::MethodPolicy,
    declaration::{
        conversion::ConversionAdapter, module::DeclarationError, native::NativeTypeConstructor,
    },
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

pub(super) mod contracts;
pub mod evidence;
pub(super) mod evidence_fingerprint;
mod members;

#[cfg(test)]
mod equivalence;
#[cfg(test)]
pub(crate) mod foundation;
#[cfg(test)]
mod host;
#[cfg(test)]
mod tests;

/// One explicitly supplied exported `.kgr` module, not a registration descriptor.
///
/// The source's module identity is authoritative for semantic ownership, and all
/// diagnostics/navigation retain its real byte ranges. Ordinary Script declarations
/// must be complete, including defaults. Setters invalidate semantic caches; retained
/// snapshots keep their exact inputs. No filesystem discovery is performed.
#[derive(Debug, Clone)]
pub struct ExternalModule {
    pub source: Arc<SourceFile>,
    pub package_alias: Option<String>,
    pub prelude: bool,
    /// Explicit library dependency identities; source still resolves every reference
    /// and checks visibility against the actual supplied declaration owner.
    pub dependencies: BTreeSet<ModuleIdentity>,
    /// Enum/variant selectors carrying failure-origin semantics absent from syntax.
    pub enum_failures: BTreeSet<(String, String)>,
    /// Exact named-import path/alias selectors with an explicit namespace category.
    pub import_namespaces: BTreeMap<(Vec<String>, Option<String>), NameNamespace>,
    /// Constructors are keyed by the exact local Native type declaration, not by
    /// use-site spelling. A user type named `String` is not a builtin.
    pub types: BTreeMap<String, ExternalType>,
    /// Host category requires a complete scoped schema/path closure, not a nominal fallback.
    pub hosts: BTreeMap<String, Arc<DefinitionMetadata<evidence::ExternalHost>>>,
    /// Scoped static conversion relationships; source owns the actual trait shape.
    pub conversions: BTreeMap<String, Arc<DefinitionMetadata<ConversionAdapter>>>,
    /// Only exceptional receiver/override policies need sidecar entries.
    pub methods: BTreeMap<ExternalFunction, MethodPolicy>,
    /// Ordered static operations and concrete interface results. IDs are scoped by
    /// each retained table and imported explicitly; no entry binding is supplied.
    pub contracts: BTreeMap<ExternalFunction, Arc<DefinitionMetadata<NativeContract>>>,
}

impl ExternalModule {
    /// Uses ordinary nominal Native types and ordinary method policies by default.
    pub fn new(source: Arc<SourceFile>) -> Self {
        Self {
            source,
            package_alias: None,
            prelude: false,
            dependencies: BTreeSet::new(),
            enum_failures: BTreeSet::new(),
            import_namespaces: BTreeMap::new(),
            types: BTreeMap::new(),
            hosts: BTreeMap::new(),
            conversions: BTreeMap::new(),
            methods: BTreeMap::new(),
            contracts: BTreeMap::new(),
        }
    }

    fn attach_types(
        &self,
        lowered: &mut LoweredModule,
        definitions: &DefinitionContext,
        cancel: &CancellationToken,
    ) -> Result<(), AnalysisError> {
        if self
            .hosts
            .keys()
            .any(|name| self.types.get(name) != Some(&ExternalType::Host))
        {
            return Err(DeclarationError(
                "external Host schema requires explicit Host category".into(),
            )
            .into());
        }
        for (name, kind) in &self.types {
            cancel.check()?;
            let mut matching = lowered
                .module
                .native_types
                .iter()
                .filter(|item| &item.name == name);
            let item = matching.next().ok_or_else(|| {
                DeclarationError(format!(
                    "external constructor names missing Native type {name}"
                ))
            })?;
            if matching.next().is_some() {
                return Err(
                    DeclarationError(format!("ambiguous external constructor {name}")).into(),
                );
            }
            let item_id = item.id;
            let host = if matches!(kind, ExternalType::Host) {
                Some(self.host_kind(name, lowered, definitions, cancel)?)
            } else {
                None
            };
            if let ExternalType::Constructor(constructor) = kind {
                let root = definitions
                    .intern_root(lowered.source.module_identity())
                    .map_err(DefinitionMappingError::from)?;
                let id = definitions
                    .intern_child(root, DefinitionKind::AssociatedType, name, 0)
                    .map_err(DefinitionMappingError::from)?;
                if NativeTypeConstructor::from_definition(id, &definitions.snapshot())
                    != Some(*constructor)
                {
                    return Err(DeclarationError(format!(
                        "invalid external constructor shape for {name}"
                    ))
                    .into());
                }
            }
            lowered.module.native_types[item_id.index()].host = host;
        }
        Ok(())
    }

    fn attach_methods(
        &self,
        lowered: &mut LoweredModule,
        cancel: &CancellationToken,
    ) -> Result<(), AnalysisError> {
        for (selector, policy) in &self.methods {
            cancel.check()?;
            let mut matching = lowered
                .module
                .functions
                .iter()
                .filter(|function| selector.matches(function, lowered));
            let function = matching.next().ok_or_else(|| {
                DeclarationError(format!(
                    "missing external method policy target {selector:?}"
                ))
            })?;
            if matching.next().is_some() {
                return Err(DeclarationError(format!(
                    "ambiguous external method policy target {selector:?}"
                ))
                .into());
            }
            if !policy.override_allowed
                && (function.kind != FunctionKind::TraitMethod
                    || function.implementation == FunctionImplementation::Required)
                || policy.receiver_access.is_some()
                    && (function.kind != FunctionKind::ImplMethod
                        || !matches!(function.implementation, FunctionImplementation::Native(_))
                        || !function
                            .params
                            .first()
                            .is_some_and(|param| param.name == "self"))
            {
                return Err(DeclarationError(format!(
                    "invalid external method policy for {selector:?}"
                ))
                .into());
            }
            let id = function.id;
            if let FunctionImplementation::Native(contract) =
                &mut lowered.module.functions[id.index()].implementation
            {
                contract.receiver_access = policy.receiver_access;
            }
            for item in &mut lowered.module.traits {
                if let Some(method) = item.methods.iter_mut().find(|method| method.function == id) {
                    method.override_allowed = policy.override_allowed;
                }
            }
        }
        Ok(())
    }
}

/// Static type interpretation absent from declaration syntax; no adapter or layout
/// implementation is supplied. Native fields remain source-owned interface facts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternalType {
    Constructor(NativeTypeConstructor),
    Host,
}

/// An exact source owner/member selector for non-syntactic static method policy.
/// Impl positions count the module's impl blocks in source order, never byte spans.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum ExternalFunction {
    Function(String),
    TraitMethod { owner: String, name: String },
    ImplMethod { owner: usize, name: String },
}

impl ExternalFunction {
    fn matches(&self, function: &Function, lowered: &LoweredModule) -> bool {
        match self {
            Self::Function(name) => function.kind == FunctionKind::User && &function.name == name,
            Self::TraitMethod { owner, name } => lowered.module.traits.iter().any(|item| {
                &item.name == owner
                    && item
                        .methods
                        .iter()
                        .any(|method| method.function == function.id && &method.name == name)
            }),
            Self::ImplMethod { owner, name } => {
                lowered.module.impls.get(*owner).is_some_and(|item| {
                    item.methods
                        .iter()
                        .any(|method| method.function == function.id && &method.name == name)
                })
            }
        }
    }
}

/// The root sidecar and its identity travel together into lowering.
pub(super) struct LibraryLowering<'a> {
    key: LibrarySourceKey,
    input: &'a ExternalModule,
    definitions: &'a DefinitionContext,
}

impl LibraryLowering<'_> {
    pub(super) fn key(&self) -> &LibrarySourceKey {
        &self.key
    }

    pub(super) fn attach(
        self,
        lowered: &mut LoweredModule,
        cancel: &CancellationToken,
    ) -> Result<(), AnalysisError> {
        self.input.attach_members(lowered, cancel)?;
        self.input.attach_types(lowered, self.definitions, cancel)?;
        self.input
            .attach_traits(lowered, self.definitions, cancel)?;
        self.input
            .attach_contracts(lowered, self.definitions, cancel)?;
        self.input.attach_methods(lowered, cancel)?;
        Ok(())
    }
}

impl AnalysisDatabase {
    /// Replaces only caller-supplied offline inputs, retaining packaged core and registrations.
    ///
    /// Text is parsed/lowered and checked with the same pipeline as project source.
    /// Metadata must name actual declarations with the declared constructor arity.
    /// Conflicting modules/files are rejected during preparation, not silently merged.
    /// These inputs can never authorize compiler admission or Runtime installation.
    pub fn set_external_modules(&mut self, modules: Vec<ExternalModule>) {
        self.libraries.set_external(modules);
        self.invalidate_library_queries();
    }

    pub(super) fn invalidate_library_queries(&mut self) {
        // Prepared registration imports and materialized views retain package spellings.
        self.native_files.take();
        self.environment_fingerprint.take();
        self.declaration_cache = None;
        self.signature_cache = None;
        self.body_cache.clear();
        self.files.clear();
    }

    pub(super) fn prepare_external(
        &mut self,
        cancel: &CancellationToken,
    ) -> Result<BTreeMap<FileId, (ParsedModule, Arc<LoweredModule>)>, AnalysisError> {
        let mut files = BTreeMap::new();
        let mut identities = HashSet::default();
        let previous = |id| {
            self.declaration_cache
                .as_ref()
                .and_then(|cache| cache.files.get(&id))
        };
        for (key, input, limits) in self.libraries.parse_inputs(self.parse_limits) {
            cancel.check()?;
            let mut cache = SourceParseCache::default();
            let mut preparer = SourcePreparer {
                cache: &mut cache,
                inline_ids: &mut self.inline_ids,
                limits,
            };
            let syntax = source_preparation::prepare_syntax(
                &mut preparer,
                &SourceSnapshot::single_file(input.source.clone()),
                |id| previous(id).map(|old| &old.parsed),
                cancel,
            )?;
            for (id, module) in syntax {
                if !identities.insert(module.source.module_identity().clone())
                    || files.contains_key(&id)
                {
                    return Err(DeclarationError(
                        "duplicate external declaration module or file".into(),
                    )
                    .into());
                }
                // Inline modules do not inherit root-owned sidecars.
                let attachments = (id == input.source.id()).then(|| LibraryLowering {
                    key: key.clone(),
                    input,
                    definitions: &self.definitions,
                });
                let prepared = module.lower(previous(id), attachments, cancel)?;
                files.insert(id, (prepared.parsed, prepared.lowered));
            }
        }
        cancel.check()?;
        Ok(files)
    }
}

//! Explicit, source-backed library inputs for offline semantic analysis.
//!
//! Source owns declarations, fields, bounds and complete Script bodies. The small
//! sidecar carries constructor, Host schema/path, foundation/conversion and operation
//! facts that Kagari syntax does not express. Neither input grants installed bindings.

use crate::{
    analysis::{
        AnalysisDatabase,
        error::AnalysisError,
        source_preparation::{self, ParsedModule, SourceParseCache, SourcePreparer},
    },
    hir::item::function::{Function, FunctionImplementation, FunctionKind, NativeContract},
    lower::LoweredModule,
    native::NativeTypeKind,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind, ModuleIdentity, map::DefinitionContext, mapping::DefinitionMappingError,
        metadata::DefinitionMetadata,
    },
};
use kagari_source::{identity::FileId, source::SourceFile, source_database::SourceSnapshot};
use kagari_types::{
    callable::MethodPolicy,
    declaration::{
        TraitDef,
        module::DeclarationError,
        native::{NativeStorageLayout, NativeTypeConstructor},
    },
};
use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    sync::Arc,
};

pub(super) mod contracts;
pub mod evidence;
pub(super) mod evidence_fingerprint;

#[cfg(test)]
mod equivalence;
#[cfg(test)]
mod foundation;
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
    /// Constructors are keyed by the exact local Native type declaration, not by
    /// use-site spelling. A user type named `Ref` or `String` is not a builtin.
    pub types: BTreeMap<String, ExternalType>,
    /// Host category requires a complete scoped schema/path closure, not a nominal fallback.
    pub hosts: BTreeMap<String, Arc<DefinitionMetadata<evidence::ExternalHost>>>,
    /// Static foundation/conversion trait shapes, checked against parsed declarations.
    /// Ordinary traits need no sidecar; their complete source owns static semantics.
    pub traits: BTreeMap<String, Arc<DefinitionMetadata<TraitDef>>>,
    /// Requests canonical foundation roles; requires explicit matching trait evidence.
    pub language_foundation: bool,
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
            types: BTreeMap::new(),
            hosts: BTreeMap::new(),
            traits: BTreeMap::new(),
            language_foundation: false,
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
            let representation = match kind {
                ExternalType::Constructor(NativeTypeConstructor::Reference(access)) => {
                    NativeTypeKind::Reference(*access)
                }
                ExternalType::Constructor(NativeTypeConstructor::String) => NativeTypeKind::String,
                ExternalType::Constructor(NativeTypeConstructor::Range(kind)) => {
                    NativeTypeKind::Range(*kind)
                }
                ExternalType::Host => self.host_kind(name, lowered, definitions, cancel)?,
                ExternalType::Storage(layout) => {
                    if !layout.valid_parameters(item.generic_params.len()) {
                        return Err(DeclarationError(format!(
                            "invalid external storage arity for {name}"
                        ))
                        .into());
                    }
                    let root = definitions
                        .intern_root(lowered.source.module_identity())
                        .map_err(DefinitionMappingError::from)?;
                    let declaration = definitions
                        .intern_child(root, DefinitionKind::AssociatedType, name, 0)
                        .map_err(DefinitionMappingError::from)?;
                    NativeTypeKind::Storage {
                        declaration,
                        arity: item.generic_params.len(),
                        layout: *layout,
                    }
                }
            };
            if representation.arity() != item.generic_params.len()
                || matches!(kind, ExternalType::Constructor(_)) && !item.fields.is_empty()
            {
                return Err(DeclarationError(format!(
                    "invalid external constructor shape for {name}"
                ))
                .into());
            }
            lowered.native_types.insert(item.id, representation);
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
            lowered.method_policies.insert(function.id, *policy);
        }
        Ok(())
    }
}

/// Static type interpretation absent from declaration syntax; no adapter or layout
/// implementation is supplied. Native fields remain source-owned interface facts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternalType {
    Constructor(NativeTypeConstructor),
    Storage(NativeStorageLayout),
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

impl AnalysisDatabase {
    /// Replaces offline library inputs, independently of installed registrations.
    ///
    /// Text is parsed/lowered and checked with the same pipeline as project source.
    /// Metadata must name actual declarations with the declared constructor arity.
    /// Conflicting modules/files are rejected during preparation, not silently merged.
    /// These inputs can never authorize compiler admission or Runtime installation.
    pub fn set_external_modules(&mut self, modules: Vec<ExternalModule>) {
        self.external_modules = modules;
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
        let mut identities = HashSet::new();
        for input in &self.external_modules {
            cancel.check()?;
            let mut cache = SourceParseCache::default();
            let preparer = SourcePreparer {
                cache: &mut cache,
                inline_ids: &mut self.inline_ids,
                limits: self.parse_limits,
            };
            for (id, mut prepared) in source_preparation::prepare(
                preparer,
                &SourceSnapshot::single_file(input.source.clone()),
                self.declaration_cache.as_ref(),
                cancel,
            )? {
                if !identities.insert(prepared.lowered.source.module_identity().clone())
                    || files.contains_key(&id)
                {
                    return Err(DeclarationError(
                        "duplicate external declaration module or file".into(),
                    )
                    .into());
                }
                // Previous declaration records are reusable only under these same
                // immutable inputs; every environment setter invalidates their cache.
                if !prepared.lowered.external_api {
                    let lowered = Arc::make_mut(&mut prepared.lowered);
                    lowered.external_api = true;
                    lowered.native_package_alias = input.package_alias.clone();
                    lowered.native_dependencies = input.dependencies.iter().cloned().collect();
                    // Inline modules retain library admission and real source ownership,
                    // but do not inherit root-owned constructor/member metadata.
                    if id == input.source.id() {
                        lowered.native_prelude = input.prelude;
                        input.attach_types(lowered, &self.definitions, cancel)?;
                        input.attach_traits(lowered, &self.definitions, cancel)?;
                        input.attach_methods(lowered, cancel)?;
                        input.attach_contracts(lowered, &self.definitions, cancel)?;
                    }
                }
                files.insert(id, (prepared.parsed, prepared.lowered));
            }
        }
        cancel.check()?;
        Ok(files)
    }
}

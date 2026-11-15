//! AST-to-HIR construction, source-site retention and uninterpreted attributes.
//!
//! [`lower_module`] is the standalone convenience entrypoint. Analysis uses the
//! already parsed AST through `lower_module_controlled`. The internal `Lowerer`
//! coordinates node allocation with [`SourceMap`]; category modules implement the
//! syntax transformations. No import lookup or type inference happens here.

use crate::{
    hir::{
        ids::{FunctionId, NativeTypeId, TraitId, VariantId},
        item::Module,
    },
    lower::context::Lowerer,
    native::{NativeBinding, NativeTypeKind},
    source_map::SourceMap,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        ModuleIdentity,
        table::{DefinitionId, DefinitionTable},
    },
};
use kagari_source::source::SourceFile;
use kagari_syntax::{
    ast::module::ModuleSyntax,
    parser::{ParseOptions, parse},
};
use kagari_types::{
    callable::MethodPolicy,
    declaration::names::NameNamespace,
    declaration::{NativeDeclaration, TraitDef, conversion::ConversionAdapter},
};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::Arc,
};

mod attribute;
pub(crate) mod context;
mod expr;
mod identity;
mod item;
mod stmt;
mod ty;

/// A source unit's owned HIR, byte-range map and registration metadata.
///
/// ```text
/// LoweredModule
/// +-- source: Arc<SourceFile>       // retained text + identity/revision
/// +-- module: Module               // declaration vectors + shared Body
/// +-- source_map: SourceMap        // matching arena, ID -> source byte sites
/// `-- registered_* / native_*      // installed declaration/provider facts
/// ```
///
/// [`lower_module`] creates ordinary source records; native preparation can attach
/// registered declaration facts. Lowering preserves incomplete shapes and unresolved
/// names: this value is neither typed nor a code-generation authorization.
#[derive(Debug, Clone)]
pub struct LoweredModule {
    /// Explicit library dependencies added to source import reachability.
    pub(crate) native_dependencies: Vec<ModuleIdentity>,
    /// Exact source traits referenced by checked native implementation metadata.
    pub(crate) native_trait_references: BTreeMap<DefinitionId, TraitDef>,
    /// Category of each authored native alias leaf, validated against its generated view.
    pub(crate) native_import_namespaces: BTreeMap<usize, NameNamespace>,
    /// Registered enum failure variants identified by lowering-local variant handles.
    pub(crate) registered_enum_failures: HashSet<VariantId>,
    /// Retained source or a source-less registration's logical graph key.
    /// Use `source_surface()` before treating this value as authored text.
    pub source: Arc<SourceFile>,
    /// Owned declaration collections and node storage.
    pub module: Module,
    /// ID-aligned source positions sharing the module body's lowering identity.
    pub source_map: SourceMap,
    /// Whether this unit was imported from an installed native declaration module.
    pub(crate) registered_native_api: bool,
    /// Parsed external declarations are static tooling inputs, never installation evidence.
    pub(crate) external_api: bool,
    /// Retained scope for attached Native registration facts; absent for ordinary source.
    pub(crate) native_definitions: Option<DefinitionTable>,
    /// Whether explicit static evidence admits this unit's language foundation roles.
    pub(crate) language_foundation: bool,
    /// Optional short package alias supplied by registration metadata.
    pub(crate) native_package_alias: Option<String>,
    /// Whether this library unit contributes implicit prelude bindings.
    pub(crate) native_prelude: bool,
    /// Authoritative installed trait contracts indexed by scoped definition identity.
    pub(crate) registered_traits: BTreeMap<DefinitionId, TraitDef>,
    /// Offline shape/conversion evidence; never installed bindings or registration authority.
    pub(crate) external_traits: BTreeMap<DefinitionId, TraitDef>,
    /// Validated installed Native contracts, separate from optional source presentation.
    pub(crate) registered_declarations: Vec<NativeDeclaration>,
    /// Native slots mapped to static type/schema interpretation, not callable authority.
    pub(crate) native_types: HashMap<NativeTypeId, NativeTypeKind>,
    /// Exact registered readonly Map/Set occurrences in the validated source view.
    /// These are concrete host descriptors, not trait capability markers.
    /// Lowering-local function slots mapped to registered semantic binding descriptors.
    pub(crate) native_functions: HashMap<FunctionId, NativeBinding>,
    /// Static conversion adapters indexed by the lowering-local trait slot.
    pub(crate) native_trait_adapters: HashMap<TraitId, ConversionAdapter>,
    /// Static receiver/call policies indexed by the lowering-local function slot.
    pub(crate) method_policies: HashMap<FunctionId, MethodPolicy>,
    /// Registered attribute byte ranges allowed on generated declaration source.
    pub(crate) native_attributes: HashSet<(usize, usize)>,
}

impl LoweredModule {
    /// Authored source when available. A structured registration retains `source`
    /// only as a logical graph key; it is not a navigation or implementation surface.
    pub fn source_surface(&self) -> Option<&Arc<SourceFile>> {
        (!self.source_map.is_structured()).then_some(&self.source)
    }

    /// Whether this module supplies an explicitly admitted library namespace.
    pub(crate) fn is_library_api(&self) -> bool {
        self.registered_native_api || self.external_api
    }

    /// Whether this module came from offline source rather than executable registration.
    pub fn is_external_api(&self) -> bool {
        self.external_api
    }

    /// Reports whether registration marks this variant as a failure case.
    pub fn registered_enum_failure(&self, variant: VariantId) -> bool {
        self.registered_enum_failures.contains(&variant)
    }

    /// Registered APIs may construct their declared generic enums from Rust;
    /// executable lowering must retain templates even without source constructors.
    pub fn is_registered_native_api(&self) -> bool {
        self.registered_native_api
    }

    /// Returns installed declarations in [`Self::registered_native_definitions`]'s scope.
    pub fn registered_native_declarations(&self) -> &[NativeDeclaration] {
        &self.registered_declarations
    }

    /// Borrow exact foreign source requirements in the retained registration scope.
    /// These facts define no source trait and grant no callable installation evidence.
    pub fn registered_source_trait_references(&self) -> &BTreeMap<DefinitionId, TraitDef> {
        &self.native_trait_references
    }

    /// Borrow the retained registration prefix; ordinary source has no attached Native scope.
    pub fn registered_native_definitions(&self) -> Option<&DefinitionTable> {
        self.native_definitions.as_ref()
    }
}

/// Parses and lowers a source file using a fresh lowering identity and default cancellation.
///
/// This convenience entrypoint returns structural HIR, not parse diagnostics or
/// checked analysis. Use [`crate::analysis::AnalysisDatabase`] for the diagnostic
/// and semantic pipeline. Missing syntax can produce placeholder nodes.
///
/// # Examples
///
/// ```
/// use kagari_hir::{hir::expr::ExprKind, lower::lower_module};
/// use kagari_source::source::SourceFile;
///
/// let source = SourceFile::new("add.kg", "fn add(x: i32) -> i32 { x + 1 }");
/// let lowered = lower_module(&source);
/// let function = &lowered.module.functions[0];
/// let block = lowered.module.block(function.implementation.script_body().unwrap());
/// let sum = block.tail_expr.unwrap();
/// let ExprKind::Binary { lhs, rhs, .. } = &lowered.module.expr(sum).kind else {
///     panic!("expected the addition node");
/// };
/// assert!(matches!(lowered.module.expr(*lhs).kind, ExprKind::Name { .. }));
/// assert!(matches!(lowered.module.expr(*rhs).kind, ExprKind::Literal(_)));
/// let span = lowered.source_map.expr_span(sum);
/// // Semantic ranges include written tokens, excluding outer whitespace and comments.
/// assert_eq!(&source.text()[span.start..span.end], "x + 1");
/// ```
/// # Panics
///
/// Panics if a manually supplied inline source extent does not identify a module
/// in its text. AnalysisDatabase validates this association with a recoverable error.
pub fn lower_module(source: &SourceFile) -> LoweredModule {
    let parsed = parse(source, ParseOptions::default(), &Default::default())
        .expect("fresh cancellation token");
    let syntax = match source.inline_range() {
        Some(range) => ModuleSyntax::at_content_range(&parsed, range)
            .expect("inline source extent must identify its syntax body"),
        None => parsed.module(),
    };
    lower_module_controlled(Arc::new(source.clone()), &syntax, &Default::default())
}

/// Lowers a logical AST view and its matching source into fresh owned HIR/source-map tables.
///
/// The caller must pair the view with its retained source revision and keep parse
/// diagnostics separately. Cancellation may leave partial structural HIR; callers
/// must check the shared token before accepting or publishing the returned module.
pub(crate) fn lower_module_controlled(
    source: Arc<SourceFile>,
    module: &ModuleSyntax,
    cancel: &CancellationToken,
) -> LoweredModule {
    let mut lowerer = Lowerer::new(cancel.clone());
    lowerer.lower_module(module);
    let (module, source_map) = lowerer.finish();
    LoweredModule::from_parts(source, module, source_map)
}

impl LoweredModule {
    pub(crate) fn from_parts(
        source: Arc<SourceFile>,
        module: Module,
        source_map: SourceMap,
    ) -> Self {
        Self {
            registered_enum_failures: HashSet::new(),
            source,
            module,
            source_map,
            registered_native_api: false,
            external_api: false,
            native_definitions: None,
            native_import_namespaces: BTreeMap::new(),
            language_foundation: false,
            native_package_alias: None,
            native_prelude: false,
            native_dependencies: Vec::new(),
            native_trait_references: BTreeMap::new(),
            registered_traits: BTreeMap::new(),
            external_traits: BTreeMap::new(),
            registered_declarations: vec![],
            native_types: HashMap::new(),
            native_functions: HashMap::new(),
            native_trait_adapters: HashMap::new(),
            method_policies: HashMap::new(),
            native_attributes: HashSet::new(),
        }
    }
}

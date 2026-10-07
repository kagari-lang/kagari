//! AST-to-HIR construction, source-site retention and uninterpreted attributes.
//!
//! [`lower_module`] is the standalone convenience entrypoint. Analysis uses the
//! already parsed AST through `lower_module_controlled`. The internal `Lowerer`
//! coordinates node allocation with [`SourceMap`]; category modules implement the
//! syntax transformations. No import lookup or type inference happens here.

use crate::{
    hir::{
        ids::{FunctionId, OpaqueTypeId, TraitId, VariantId},
        item::Module,
    },
    lower::context::Lowerer,
    native::{NativeBinding, NativeTypeKind},
    source_map::SourceMap,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionPath, ModuleIdentity},
    span::Span,
};
use kagari_source::source::SourceFile;
use kagari_syntax::{
    ast::{
        item::{
            Attribute, AttributeArg, AttributeValue as AstAttributeValue,
            SourceFile as AstSourceFile,
        },
        traits::AstNode,
    },
    parser::parse,
};
use kagari_types::{
    callable::MethodPolicy,
    collection::CollectionAccess,
    declaration::{NativeDeclaration, TraitDef, conversion::ConversionAdapter},
};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::Arc,
};

pub(crate) mod context;
mod expr;
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
/// +-- attributes: [AttributeFact]  // syntax metadata, not executed code
/// `-- registered_* / native_*      // installed declaration/provider facts
/// ```
///
/// [`lower_module`] creates ordinary source records; native preparation can attach
/// registered declaration facts. Lowering preserves incomplete shapes and unresolved
/// names: this value is neither typed nor a code-generation authorization.
#[derive(Debug, Clone)]
pub struct LoweredModule {
    /// Installed declaration dependencies added to source import reachability.
    pub(crate) native_dependencies: Vec<ModuleIdentity>,
    /// Registered enum failure variants identified by lowering-local variant handles.
    pub(crate) registered_enum_failures: HashSet<VariantId>,
    /// Source retained for this lowering; source-map byte offsets refer to this text.
    pub source: Arc<SourceFile>,
    /// Owned declaration collections and node storage.
    pub module: Module,
    /// ID-aligned source positions sharing the module body's arena.
    pub source_map: SourceMap,
    /// Structured source attributes with attribute and target ranges.
    pub attributes: Vec<AttributeFact>,
    /// Whether this unit was imported from an installed native declaration module.
    pub(crate) registered_native_api: bool,
    /// Whether the installed unit provides the required language foundation.
    pub(crate) language_foundation: bool,
    /// Optional short package alias supplied by registration metadata.
    pub(crate) native_package_alias: Option<String>,
    /// Whether this registered unit contributes implicit prelude bindings.
    pub(crate) native_prelude: bool,
    /// Registered array bridge interface identities indexed by access policy.
    pub(crate) native_array_interfaces: BTreeMap<CollectionAccess, DefinitionPath>,
    /// Authoritative installed trait contracts indexed by portable definition identity.
    pub(crate) registered_traits: BTreeMap<DefinitionPath, TraitDef>,
    /// Validated native declaration records retained alongside their generated source view.
    pub(crate) registered_declarations: Vec<NativeDeclaration>,
    /// Lowering-local opaque type slots mapped to registered representation descriptors.
    pub(crate) native_types: HashMap<OpaqueTypeId, NativeTypeKind>,
    /// Lowering-local function slots mapped to registered semantic binding descriptors.
    pub(crate) native_functions: HashMap<FunctionId, NativeBinding>,
    /// Registered conversion adapters indexed by the generated trait slot.
    pub(crate) native_trait_adapters: HashMap<TraitId, ConversionAdapter>,
    /// Collection access policies indexed by registered trait slot.
    pub(crate) native_trait_access: HashMap<TraitId, CollectionAccess>,
    /// Registered receiver/call policies indexed by generated function slot.
    pub(crate) method_policies: HashMap<FunctionId, MethodPolicy>,
    /// Registered attribute byte ranges allowed on generated declaration source.
    pub(crate) native_attributes: HashSet<(usize, usize)>,
}

impl LoweredModule {
    /// Reports whether registration marks this variant as a failure case.
    pub fn registered_enum_failure(&self, variant: VariantId) -> bool {
        self.registered_enum_failures.contains(&variant)
    }

    /// Registered APIs may construct their declared generic enums from Rust;
    /// executable lowering must retain templates even without source constructors.
    pub fn is_registered_native_api(&self) -> bool {
        self.registered_native_api
    }

    /// Returns the installed semantic declarations attached during native preparation.
    pub fn registered_native_declarations(&self) -> &[NativeDeclaration] {
        &self.registered_declarations
    }
}

/// A lowered attribute attached to a source item, without interpretation of its policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributeFact {
    /// Attribute name as written; recovery may produce an empty name.
    pub name: String,
    /// Assigned value, when the attribute uses a value form.
    pub value: Option<AttributeValue>,
    /// Parenthesized arguments; `None` means no argument-list syntax.
    pub arguments: Option<Vec<AttributeArgument>>,
    /// Byte range of the entire attribute.
    pub span: Span,
    /// Byte range of the syntax node to which the attribute is attached.
    pub target_span: Span,
}

/// A positional or named value within an attribute argument/list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributeArgument {
    /// Argument key; absent for a positional argument.
    pub name: Option<String>,
    /// Literal, path, nested list or missing recovered value.
    pub value: AttributeValue,
}

/// Attribute syntax retained for consumers rather than evaluated during lowering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttributeValue {
    /// Literal source spelling, including its original delimiters/suffix.
    Literal(String),
    /// Unresolved source path spelling.
    Path(String),
    /// Nested positional/named elements in source order.
    List(Vec<AttributeArgument>),
    /// An argument whose value was absent in recovered syntax.
    Missing,
}

fn attribute_argument(arg: AttributeArg) -> AttributeArgument {
    let value = match arg.value() {
        Some(value) => attribute_value(value),
        None => AttributeValue::Missing,
    };
    AttributeArgument {
        name: arg.name_text(),
        value,
    }
}

fn attribute_value(value: AstAttributeValue) -> AttributeValue {
    if let Some(literal) = value.literal() {
        AttributeValue::Literal(literal.text().unwrap_or_default())
    } else if let Some(path) = value.path() {
        AttributeValue::Path(path.text().unwrap_or_default())
    } else {
        AttributeValue::List(value.elements().map(attribute_argument).collect())
    }
}

pub(crate) fn lower_attributes(module: &AstSourceFile) -> Vec<AttributeFact> {
    module
        .syntax()
        .descendants()
        .filter_map(Attribute::cast)
        .filter_map(|attribute| {
            let parent = attribute.syntax().parent()?;
            let target = parent.text_range();
            Some(AttributeFact {
                name: attribute.name_text().unwrap_or_default(),
                value: attribute.value().map(attribute_value),
                arguments: attribute
                    .args()
                    .map(|args| args.arguments().map(attribute_argument).collect()),
                span: context::syntax_span(&attribute),
                target_span: Span::new(usize::from(target.start()), usize::from(target.end())),
            })
        })
        .collect()
}

/// Parses and lowers a source file using a fresh arena and default cancellation.
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
/// let block = lowered.module.block(function.body.unwrap());
/// let sum = block.tail_expr.unwrap();
/// let ExprKind::Binary { lhs, rhs, .. } = &lowered.module.expr(sum).kind else {
///     panic!("expected the addition node");
/// };
/// assert!(matches!(lowered.module.expr(*lhs).kind, ExprKind::Name { .. }));
/// assert!(matches!(lowered.module.expr(*rhs).kind, ExprKind::Literal(_)));
/// let span = lowered.source_map.expr_span(sum);
/// // Full node ranges can include trailing trivia inherited from the syntax tree.
/// assert_eq!(&source.text()[span.start..span.end], "x + 1 ");
/// ```
pub fn lower_module(source: &SourceFile) -> LoweredModule {
    let parsed = parse(source);
    lower_module_controlled(
        Arc::new(source.clone()),
        &parsed.syntax(),
        &Default::default(),
    )
}

/// Lowers an already parsed AST and supplied source together; callers retain diagnostics and check cancellation.
pub(crate) fn lower_module_controlled(
    source: Arc<SourceFile>,
    module: &AstSourceFile,
    cancel: &CancellationToken,
) -> LoweredModule {
    let attributes = lower_attributes(module);
    let mut lowerer = Lowerer::new(cancel.clone());
    lowerer.lower_module(module);
    let (module, source_map) = lowerer.finish();
    LoweredModule {
        registered_enum_failures: HashSet::new(),
        source,
        module,
        source_map,
        attributes,
        registered_native_api: false,
        language_foundation: false,
        native_package_alias: None,
        native_prelude: false,
        native_dependencies: Vec::new(),
        native_array_interfaces: BTreeMap::new(),
        registered_traits: BTreeMap::new(),
        registered_declarations: vec![],
        native_types: HashMap::new(),
        native_functions: HashMap::new(),
        native_trait_access: HashMap::new(),
        native_trait_adapters: HashMap::new(),
        method_policies: HashMap::new(),
        native_attributes: HashSet::new(),
    }
}

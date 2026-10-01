use ast::Attribute;
use kagari_abi::callable::MethodPolicy;
use kagari_common::{SourceFile, Span, cancellation::CancellationToken, identity::DefinitionId};
use kagari_stdlib::ParsedStdlibPackage;
use kagari_syntax::parse;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
pub(crate) mod context;
mod expr;
mod item;
mod stmt;
mod ty;

use kagari_syntax::ast::{self, AstNode};

use crate::{
    hir::{EnumId, FunctionId, Module, OpaqueTypeId},
    lower::context::Lowerer,
    native::NativeTypeKind,
    source_map::SourceMap,
};

#[derive(Debug, Clone)]
pub struct LoweredModule {
    pub source: Arc<SourceFile>,
    pub module: Module,
    pub source_map: SourceMap,
    pub attributes: Vec<AttributeFact>,
    pub(crate) registered_native_api: bool,
    pub(crate) native_types: HashMap<OpaqueTypeId, NativeTypeKind>,
    pub(crate) native_enums: HashMap<EnumId, NativeTypeKind>,
    pub(crate) native_functions: HashMap<FunctionId, DefinitionId>,
    pub(crate) method_policies: HashMap<FunctionId, MethodPolicy>,
    pub(crate) native_attributes: HashSet<(usize, usize)>,
    pub(crate) installed_stdlib: Option<Arc<ParsedStdlibPackage>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributeFact {
    pub name: String,
    pub arguments: Option<Vec<AttributeArgument>>,
    pub span: Span,
    pub target_span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributeArgument {
    pub name: Option<String>,
    pub value: AttributeValue,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttributeValue {
    Literal(String),
    Path(String),
    List(Vec<AttributeArgument>),
    Missing,
}

fn attribute_argument(arg: ast::AttributeArg) -> AttributeArgument {
    let value = match arg.value() {
        Some(value) => {
            if let Some(literal) = value.literal() {
                AttributeValue::Literal(literal.text().unwrap_or_default())
            } else if let Some(path) = value.path() {
                AttributeValue::Path(path.text().unwrap_or_default())
            } else {
                AttributeValue::List(value.elements().map(attribute_argument).collect())
            }
        }
        None => AttributeValue::Missing,
    };
    AttributeArgument {
        name: arg.name_text(),
        value,
    }
}

fn lower_attributes(module: &ast::SourceFile) -> Vec<AttributeFact> {
    module
        .syntax()
        .descendants()
        .filter_map(Attribute::cast)
        .filter_map(|attribute| {
            let parent = attribute.syntax().parent()?;
            let target = parent.text_range();
            Some(AttributeFact {
                name: attribute.name_text().unwrap_or_default(),
                arguments: attribute
                    .args()
                    .map(|args| args.arguments().map(attribute_argument).collect()),
                span: context::syntax_span(&attribute),
                target_span: Span::new(usize::from(target.start()), usize::from(target.end())),
            })
        })
        .collect()
}

pub fn lower_module(source: &SourceFile) -> LoweredModule {
    let parsed = parse(source);
    lower_module_controlled(
        Arc::new(source.clone()),
        &parsed.syntax(),
        &Default::default(),
    )
}

pub(crate) fn lower_module_controlled(
    source: Arc<SourceFile>,
    module: &ast::SourceFile,
    cancel: &CancellationToken,
) -> LoweredModule {
    let attributes = lower_attributes(module);
    let mut lowerer = Lowerer::new(cancel.clone());
    lowerer.lower_module(module);
    let (module, source_map) = lowerer.finish();
    LoweredModule {
        source,
        module,
        source_map,
        attributes,
        registered_native_api: false,
        native_types: HashMap::new(),
        native_enums: HashMap::new(),
        native_functions: HashMap::new(),
        method_policies: HashMap::new(),
        native_attributes: HashSet::new(),
        installed_stdlib: None,
    }
}

mod context;
mod expr;
mod item;
mod stmt;
mod ty;

use kagari_syntax::ast::{self, AstNode};

use crate::hir::Module;
use crate::source_map::SourceMap;

use crate::lower::context::Lowerer;

#[derive(Debug, Clone)]
pub struct LoweredModule {
    pub source: std::sync::Arc<kagari_common::SourceFile>,
    pub module: Module,
    pub source_map: SourceMap,
    pub attributes: Vec<AttributeFact>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributeFact {
    pub name: String,
    pub arguments: Option<Vec<AttributeArgument>>,
    pub span: kagari_common::Span,
    pub target_span: kagari_common::Span,
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
        .filter_map(ast::Attribute::cast)
        .filter_map(|attribute| {
            let parent = attribute.syntax().parent()?;
            let target = parent.text_range();
            Some(AttributeFact {
                name: attribute.name_text().unwrap_or_default(),
                arguments: attribute
                    .args()
                    .map(|args| args.arguments().map(attribute_argument).collect()),
                span: context::syntax_span(&attribute),
                target_span: kagari_common::Span::new(
                    usize::from(target.start()),
                    usize::from(target.end()),
                ),
            })
        })
        .collect()
}

pub fn lower_module(source: &kagari_common::SourceFile) -> LoweredModule {
    let parsed = kagari_syntax::parse(source);
    lower_module_controlled(
        std::sync::Arc::new(source.clone()),
        &parsed.syntax(),
        &Default::default(),
    )
}

pub(crate) fn lower_module_controlled(
    source: std::sync::Arc<kagari_common::SourceFile>,
    module: &ast::SourceFile,
    cancel: &kagari_common::cancellation::CancellationToken,
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
    }
}

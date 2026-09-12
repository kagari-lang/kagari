mod core;
mod grammar;

use kagari_common::SourceFile;
use kagari_common::cancellation::{CancellationToken, Cancelled};
use rowan::GreenNode;

use crate::{
    BoxedDiagnosticBuffer, DiagnosticBuffer,
    ast::{self, AstNode},
    lexer::lex_with_cancellation,
    syntax_node::syntax_node_from_green,
};

pub(crate) use core::Parser;

#[derive(Debug, Clone)]
pub struct Parse {
    green: GreenNode,
    diagnostics: DiagnosticBuffer,
}

impl Parse {
    pub fn syntax(&self) -> ast::SourceFile {
        ast::SourceFile::cast(syntax_node_from_green(self.green.clone()))
            .expect("parser must always produce a source file node")
    }

    pub fn diagnostics(&self) -> &DiagnosticBuffer {
        &self.diagnostics
    }
}

pub fn parse(source: &SourceFile) -> Parse {
    parse_with_cancellation(source, &CancellationToken::default())
        .expect("fresh cancellation token")
}

pub fn parse_with_cancellation(
    source: &SourceFile,
    cancel: &CancellationToken,
) -> Result<Parse, Cancelled> {
    let tokens = lex_with_cancellation(source.text(), cancel)?;
    let mut parser = Parser::new(source.text(), tokens, cancel.clone());
    parser.parse_root();
    let (green, diagnostics) = parser.finish();
    cancel.check()?;
    Ok(Parse { green, diagnostics })
}

pub fn parse_module(source: &SourceFile) -> Result<ast::SourceFile, BoxedDiagnosticBuffer> {
    let parse = parse(source);
    if !parse.diagnostics.is_empty() {
        return Err(Box::new(parse.diagnostics));
    }
    Ok(parse.syntax())
}

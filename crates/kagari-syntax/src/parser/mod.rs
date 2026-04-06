mod core;
mod grammar;

use kagari_common::SourceFile;
use rowan::GreenNode;

use crate::{
    BoxedDiagnosticBuffer, DiagnosticBuffer,
    ast::{self, AstNode},
    lexer::lex,
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
    let tokens = lex(source.text());
    let mut parser = Parser::new(source.text(), tokens);
    parser.parse_root();
    let (green, diagnostics) = parser.finish();
    Parse { green, diagnostics }
}

pub fn parse_module(source: &SourceFile) -> Result<ast::SourceFile, BoxedDiagnosticBuffer> {
    let parse = parse(source);
    if !parse.diagnostics.is_empty() {
        return Err(Box::new(parse.diagnostics));
    }
    Ok(parse.syntax())
}

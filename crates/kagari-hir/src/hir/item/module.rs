use crate::hir::ModuleId;
use kagari_common::Span;

use super::Visibility;

#[derive(Debug, Clone)]
pub struct ModuleDecl {
    pub id: ModuleId,
    pub visibility: Visibility,
    pub name: String,
    pub inline: bool,
}

#[derive(Debug, Clone)]
pub struct Import {
    pub visibility: Visibility,
    pub alias: String,
    pub path: String,
    pub span: Span,
    pub glob: bool,
}

pub type ModuleDeclBuffer = Vec<ModuleDecl>;
pub type ImportBuffer = Vec<Import>;

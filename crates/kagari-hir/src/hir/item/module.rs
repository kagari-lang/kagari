use crate::{builtin::surface, hir::ModuleId};

use super::Visibility;

#[derive(Debug, Clone)]
pub struct ModuleDecl {
    pub id: ModuleId,
    pub visibility: Visibility,
    pub name: String,
    pub inline: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StandardImportTarget {
    Module(surface::StandardModule),
    Function(surface::StandardIntrinsic),
}

#[derive(Debug, Clone)]
pub struct StandardImport {
    pub visibility: Visibility,
    pub alias: String,
    pub target: StandardImportTarget,
}

pub type ModuleDeclBuffer = Vec<ModuleDecl>;
pub type StandardImportBuffer = Vec<StandardImport>;

use crate::hir::ModuleId;

use super::Visibility;

#[derive(Debug, Clone)]
pub struct ModuleDecl {
    pub id: ModuleId,
    pub visibility: Visibility,
    pub name: String,
    pub inline: bool,
}

pub type ModuleDeclBuffer = Vec<ModuleDecl>;

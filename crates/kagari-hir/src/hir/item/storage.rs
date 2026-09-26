use crate::hir::{ConstId, EnumId, ExprId, FunctionId, ModuleId, StructId, TraitId, TypeRefId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    Private,
    PublicSuper,
    Public,
}

impl Visibility {
    pub fn allows(
        self,
        owner: &kagari_common::identity::ModuleIdentity,
        accessor: &kagari_common::identity::ModuleIdentity,
    ) -> bool {
        if self == Self::Public {
            return true;
        }
        if owner.package != accessor.package {
            return false;
        }
        let scope = match self {
            Self::Private => owner.path.len(),
            Self::PublicSuper => owner.path.len().saturating_sub(1),
            Self::Public => unreachable!(),
        };
        accessor.path.starts_with(&owner.path[..scope])
    }
}

#[derive(Debug, Clone)]
pub struct ConstItem {
    pub id: ConstId,
    pub visibility: Visibility,
    pub name: String,
    pub ty: Option<TypeRefId>,
    pub initializer: ExprId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExportItem {
    Function(FunctionId),
    Const(ConstId),
    Module(ModuleId),
    Import(usize),
    Struct(StructId),
    Enum(EnumId),
    Trait(TraitId),
}

#[derive(Debug, Clone)]
pub struct Export {
    pub name: String,
    pub item: ExportItem,
}

pub type ConstBuffer = Vec<ConstItem>;
pub type ExportBuffer = Vec<Export>;

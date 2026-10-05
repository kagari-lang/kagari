//! Declared module visibility, shared by source analysis and executable metadata.
use kagari_common::identity::ModuleIdentity;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Visibility {
    Private,
    PublicSuper,
    Public,
}

impl Visibility {
    pub fn allows(self, owner: &ModuleIdentity, accessor: &ModuleIdentity) -> bool {
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

//! Declared module visibility, shared by source analysis and executable metadata.
use kagari_common::identity::ModuleIdentity;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Visibility {
    Private,
    PublicSuper,
    Public,
    PublicCrate,
}

impl Visibility {
    pub fn allows(self, owner: &ModuleIdentity, accessor: &ModuleIdentity) -> bool {
        if self == Self::Public {
            return true;
        }
        if owner.package != accessor.package {
            return false;
        }
        accessor.path.starts_with(self.scope(owner))
    }

    /// Whether a re-export's complete access scope stays within this declaration's scope.
    pub fn allows_reexport(
        self,
        owner: &ModuleIdentity,
        exported: Self,
        exporter: &ModuleIdentity,
    ) -> bool {
        if self == Self::Public {
            return true;
        }
        if exported == Self::Public || owner.package != exporter.package {
            return false;
        }
        exported.scope(exporter).starts_with(self.scope(owner))
    }

    fn scope(self, owner: &ModuleIdentity) -> &[String] {
        let length = match self {
            Self::Private => owner.path.len(),
            Self::PublicSuper => owner.path.len().saturating_sub(1),
            Self::PublicCrate => 0,
            Self::Public => unreachable!(),
        };
        &owner.path[..length]
    }
}

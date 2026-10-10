//! Applied members retain complete portable identity at their type-preparation boundary.
use crate::{error::RuntimeError, module::EnumVariantRef};
use kagari_common::identity::DefinitionPath;

#[derive(Debug)]
pub(crate) struct PreparedEnumMember {
    layout: EnumVariantRef,
    identity: DefinitionPath,
}

impl PreparedEnumMember {
    pub(super) fn new(layout: EnumVariantRef) -> Result<Self, RuntimeError> {
        let identity = layout
            .module()
            .definition(layout.variant().declaration)?
            .to_path();
        Ok(Self { layout, identity })
    }

    pub(crate) fn layout(&self) -> &EnumVariantRef {
        &self.layout
    }

    pub(crate) fn name(&self) -> Option<&str> {
        self.identity.path.last().map(|member| member.name.as_str())
    }

    pub(crate) fn matches_identity(&self, identity: &DefinitionPath) -> bool {
        // Authoring handles remain portable across runtimes. Compare the complete
        // identity, including module, segment kinds and occurrences, not only a name.
        // The retained applied layout separately owns payload scope and generation.
        &self.identity == identity
    }
}

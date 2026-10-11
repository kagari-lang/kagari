//! Applied enum members retain their generation-pinned layout and scoped identity.
use crate::{error::RuntimeError, module::EnumVariantRef};
use kagari_common::identity::table::{DefinitionId, DefinitionTable};

#[derive(Debug)]
pub(crate) struct PreparedEnumMember {
    layout: EnumVariantRef,
}

impl PreparedEnumMember {
    pub(super) fn new(layout: EnumVariantRef) -> Result<Self, RuntimeError> {
        layout.module().definition(layout.variant().declaration)?;
        Ok(Self { layout })
    }

    pub(crate) fn layout(&self) -> &EnumVariantRef {
        &self.layout
    }

    pub(crate) fn name(&self) -> Option<&str> {
        self.layout
            .module()
            .definition(self.layout.variant().declaration)
            .ok()?
            .last()
            .map(|member| member.name)
    }

    pub(crate) fn matches_identity(
        &self,
        identity: DefinitionId,
        definitions: &DefinitionTable,
    ) -> bool {
        let Ok(actual) = self
            .layout
            .module()
            .definition(self.layout.variant().declaration)
        else {
            return false;
        };
        let Ok(expected) = definitions.resolve(identity) else {
            return false;
        };
        actual.module() == expected.module() && actual.segments().eq(expected.segments())
    }
}

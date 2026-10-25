//! Compilation-owned identities for generated bridges across lowering rounds.
use crate::source::lower::MirLoweringError;
use kagari_common::identity::{
    DefinitionKind, map::DefinitionContext, mapping::DefinitionMappingError, table::DefinitionId,
};
use kagari_hir::types::{NominalType, TypeId};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) enum BridgeKey {
    Intrinsic(TypeId, NominalType),
    Tuple(usize, NominalType),
}

/// One immutable checked source closure owns this map until its fixed point.
/// Replanning may discover bridges in another order, but cannot retarget an ID
/// already present in retained executable evidence. Independent compilations
/// keep independent maps; this is not an execution cache or proof authority.
#[derive(Default)]
pub(crate) struct BridgeIdentities {
    modules: HashMap<DefinitionId, HashMap<BridgeKey, DefinitionId>>,
}

impl BridgeIdentities {
    pub(super) fn intern(
        &mut self,
        context: &DefinitionContext,
        root: DefinitionId,
        authored: usize,
        key: BridgeKey,
    ) -> Result<DefinitionId, MirLoweringError> {
        let entries = self.modules.entry(root).or_default();
        if let Some(id) = entries.get(&key) {
            return Ok(*id);
        }
        let occurrence = authored
            .checked_add(entries.len())
            .and_then(|value| u32::try_from(value).ok())
            .ok_or(MirLoweringError::MissingBinding(
                "interface bridge identity limit",
            ))?;
        let id = context
            .intern_child(root, DefinitionKind::Impl, "", occurrence)
            .map_err(DefinitionMappingError::from)?;
        entries.insert(key, id);
        Ok(id)
    }
}

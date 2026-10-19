//! Definition-independent callee storage and parameter placement.
use crate::module::execution::layout::FrameLayout;
use kagari_bytecode::module::BytecodeFunction;
use kagari_contract::native_import::NativeImport;
use std::sync::Arc;

/// Reusable callee facts, separate from any caller's source/return locations.
/// Generic binder identities stay in the normalized runtime contract.
#[derive(Debug)]
pub(crate) struct CalleeLayout {
    pub slot_layout: Arc<FrameLayout>,
    pub parameter_slots: Box<[usize]>,
    pub parameter_count: usize,
    pub generic: bool,
    pub may_suspend: bool,
}

impl CalleeLayout {
    pub(super) fn script(function: &BytecodeFunction, work: &mut usize) -> Self {
        Self {
            slot_layout: Arc::new(FrameLayout::prepare(function, work)),
            parameter_slots: function
                .metadata
                .parameter_slots
                .iter()
                .map(|slot| slot.index())
                .collect(),
            parameter_count: usize::from(function.parameter_count),
            generic: function.metadata.semantic.generic.is_some(),
            may_suspend: function.metadata.effects.may_suspend,
        }
    }

    pub(super) fn native(import: &NativeImport) -> Self {
        Self {
            slot_layout: Arc::new(FrameLayout::native(import)),
            parameter_slots: (1..=import.signature.params.len()).collect(),
            parameter_count: import.signature.params.len(),
            generic: import.generic.is_some(),
            may_suspend: false,
        }
    }
}

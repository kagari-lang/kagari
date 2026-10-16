//! Definition-independent callee storage and parameter placement.
use crate::module::execution::layout::FrameLayout;
use kagari_bytecode::module::BytecodeFunction;
use kagari_contract::native_import::NativeImport;
use std::sync::Arc;

/// Reusable callee facts, separate from any caller's source/return locations.
/// Generic binder identities stay in the normalized runtime contract.
#[derive(Debug)]
pub(crate) struct CalleeLayout {
    pub registers: Arc<FrameLayout>,
    pub argument_offset: usize,
    pub parameter_count: usize,
    pub generic: bool,
    pub may_suspend: bool,
}

impl CalleeLayout {
    pub(super) fn script(function: &BytecodeFunction, work: &mut usize) -> Self {
        Self {
            registers: Arc::new(FrameLayout::prepare(function, work)),
            argument_offset: usize::from(function.register_count),
            parameter_count: usize::from(function.parameter_count),
            generic: function.metadata.semantic.generic.is_some(),
            may_suspend: function.metadata.effects.may_suspend,
        }
    }

    pub(super) fn native(import: &NativeImport) -> Self {
        Self {
            registers: Arc::new(FrameLayout::native(import)),
            argument_offset: 1,
            parameter_count: import.signature.params.len(),
            generic: import.generic.is_some(),
            may_suspend: false,
        }
    }
}

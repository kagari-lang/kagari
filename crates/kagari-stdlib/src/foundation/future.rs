//! Cold Future storage and its declaration belong to core::future.
use crate::foundation::type_docs;
use kagari_runtime::native::{
    assembly::draft::ModuleDraft, binding::NativeResult, storage::NativeStorage,
};
use kagari_types::declaration::{TypeDefKind, native::NativeStorageLayout};

pub(super) const MODULE_DOCS: (&str, &str) = (
    "Cold asynchronous operations. Creation captures inputs without starting work; first await drives the operation once. Aliases share that single-drive state.",
    "async fn main() -> bool {\n    val calls: Vec<i32> = Vec::new();\n    val future = async { calls.push(1); 42 };\n    val cold = calls.is_empty();\n    val value = future.await;\n    cold && value == 42 && calls.len() == 1u64\n}",
);

const TYPE_DOCS: (&str, &str) = (
    "A cold runtime-local operation with one invariant result type. Creation does not submit work. Await drives once and returns the result; repeated awaiting through any alias traps. Cancellation terminates execution rather than wrapping the result in Result.\n\nThe example is driven by the host interpreter. Observe that creating the cold\noperation does not run its body, and the single await performs the effect.",
    MODULE_DOCS.1,
);

pub(super) fn declare(module: &mut ModuleDraft<'_>) -> NativeResult<()> {
    let future = module.define_type(
        "Future",
        TypeDefKind::NativeStorage(NativeStorageLayout::Future),
        1,
        |_, _| Ok(()),
    )?;
    module.bind_storage(&future, NativeStorage::future())?;
    type_docs(module, &future, "Future", TYPE_DOCS, &[])
}

//! Host reference constructors and scope-bound capability documentation.
use crate::foundation::type_docs;
use kagari_runtime::native::{assembly::draft::ModuleDraft, binding::NativeResult};
use kagari_types::{declaration::native::NativeTypeConstructor, reference::ReferenceAccess};

pub(super) fn declare(module: &mut ModuleDraft<'_>) -> NativeResult<()> {
    for (access, docs) in [
        (ReferenceAccess::Shared, SHARED_DOCS),
        (ReferenceAccess::Mutable, MUTABLE_DOCS),
    ] {
        let reference =
            module.define_constructor(access.name(), NativeTypeConstructor::Reference(access))?;
        type_docs(module, &reference, access.name(), docs, &[])?;
    }
    Ok(())
}

const SHARED_DOCS: (&str, &str) = (
    "A shared host location valid only within its execution scope. It cannot be returned, captured, stored in owning values or retained across suspension. The target schema does not grant a Rust reference without an installed host capability.\n\nThis declaration-only example forwards an existing scoped view to a synchronous visitor. It does not create a Player or a host capability. The host must install the target schema and supply the view within a checked execution scope; this is not a standalone program.",
    "struct Player {}\nfn inspect(value: Ref<Player>, visitor: fn(Ref<Player>) -> i32) -> i32 {\n    visitor(value)\n}",
);
const MUTABLE_DOCS: (&str, &str) = (
    "A mutable host location valid only within its execution scope. Exclusive access uses checked loans; holding a view neither transfers ownership nor creates an active Rust reference. A shared view cannot upgrade to this capability.\n\nThis declaration-only example passes an existing mutable view to a synchronous visitor. The host supplies both the scoped capability and the visitor; declaring Player alone creates neither. No view is stored or returned.",
    "struct Player {}\nfn update(value: RefMut<Player>, visitor: fn(RefMut<Player>) -> i32) -> i32 {\n    visitor(value)\n}",
);

pub(super) const MODULE_DOCS: (&str, &str) = (
    "Scope-bound shared and mutable host locations with checked target and access capabilities. These views retain no host ownership.",
    "struct Player {}\nfn inspect(value: Ref<Player>, visitor: fn(Ref<Player>) -> i32) -> i32 {\n    visitor(value)\n}",
);

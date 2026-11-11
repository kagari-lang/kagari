mod runtime_boundary;

mod documentation;

mod registration;

// Shared raw fixture proof preparation; ordinary stdlib links the canonical Runtime.
#[path = "../../kagari-runtime/tests/support/bytecode.rs"]
mod bytecode;

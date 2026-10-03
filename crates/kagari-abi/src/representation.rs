use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ValueType {
    /// No runtime value can inhabit this representation.
    Never,
    #[default]
    Unit,
    Bool,
    I32,
    I64,
    U64,
    F32,
    F64,
    Str,
    // Execution-layer reference to a heap-backed runtime object. This is intentionally
    // broader than concrete semantic types and covers tuples, arrays, structs, enums, and
    // future runtime-managed objects such as closures or reflected values.
    HeapObject,
    HostHandle,
    /// Tagged runtime Value in a checked shared generic body. Its semantic type
    /// is a scoped parameter or projection, supplied by the call environment.
    Generic,
}

impl ValueType {
    pub fn may_contain_gc_reference(self) -> bool {
        matches!(self, Self::HeapObject | Self::Generic)
    }
}

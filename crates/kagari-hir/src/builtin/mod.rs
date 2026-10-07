//! Bounded syntax/helper bridges and intrinsic scalar capabilities; ordinary libraries remain registered inputs.

pub mod array_bridge;
pub mod surface;

/// Small compiler-recognized helper surface; ordinary library functions use registered declarations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuiltinFunction {
    /// The `type_of` helper surface for declared type metadata.
    TypeOf,
    /// The `get_field` helper surface, subject to checked field access.
    GetField,
    /// The `set_field` helper surface, subject to checked mutation access.
    SetField,
    /// The `set_index` helper surface, subject to checked index mutation.
    SetIndex,
    /// The `print` helper surface.
    Print,
}

impl BuiltinFunction {
    pub(crate) fn from_name(name: &str) -> Option<Self> {
        match name {
            "type_of" => Some(Self::TypeOf),
            "get_field" => Some(Self::GetField),
            "set_field" => Some(Self::SetField),
            "set_index" => Some(Self::SetIndex),
            "print" => Some(Self::Print),
            _ => None,
        }
    }
}

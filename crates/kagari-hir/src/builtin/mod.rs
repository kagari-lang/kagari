pub mod surface;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuiltinFunction {
    TypeOf,
    GetField,
    SetField,
    SetIndex,
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

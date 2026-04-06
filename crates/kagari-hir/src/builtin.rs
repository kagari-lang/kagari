#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinFunction {
    TypeOf,
    GetField,
    SetField,
    SetIndex,
}

impl BuiltinFunction {
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "type_of" => Some(Self::TypeOf),
            "get_field" => Some(Self::GetField),
            "set_field" => Some(Self::SetField),
            "set_index" => Some(Self::SetIndex),
            _ => None,
        }
    }
}

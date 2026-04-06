use crate::host::HostObjectId;

#[derive(Debug, Clone, PartialEq)]
pub struct StructValueField {
    pub name: String,
    pub value: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Unit,
    Bool(bool),
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
    Str(String),
    Tuple(Vec<Value>),
    Array(Vec<Value>),
    Struct {
        name: String,
        fields: Vec<StructValueField>,
    },
    GcHandle(u64),
    HostRef(HostObjectId),
    HostMut(HostObjectId),
}

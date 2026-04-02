use crate::host::HostObjectId;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Unit,
    Bool(bool),
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
    Str(String),
    GcHandle(u64),
    HostRef(HostObjectId),
    HostMut(HostObjectId),
}

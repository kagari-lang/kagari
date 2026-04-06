use kagari_sema::{BuiltinType, TypeId};

use crate::Instruction;

#[derive(Debug, Clone)]
pub struct IrModule {
    pub functions: FunctionBuffer,
}

#[derive(Debug, Clone)]
pub struct IrFunction {
    pub name: String,
    pub params: ParameterBuffer,
    pub return_type: ValueType,
    pub code: InstructionBuffer,
}

#[derive(Debug, Clone)]
pub struct IrParameter {
    pub name: String,
    pub ty: ValueType,
}

pub type FunctionBuffer = smallvec::SmallVec<[IrFunction; 8]>;
pub type ParameterBuffer = smallvec::SmallVec<[IrParameter; 4]>;
pub type InstructionBuffer = smallvec::SmallVec<[Instruction; 16]>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueType {
    Unit,
    Bool,
    I32,
    I64,
    F32,
    F64,
    Str,
}

impl ValueType {
    pub fn from_type_id(type_id: TypeId) -> Self {
        match type_id {
            TypeId::Builtin(BuiltinType::Unit) => Self::Unit,
            TypeId::Builtin(BuiltinType::Bool) => Self::Bool,
            TypeId::Builtin(BuiltinType::I32) => Self::I32,
            TypeId::Builtin(BuiltinType::I64) => Self::I64,
            TypeId::Builtin(BuiltinType::F32) => Self::F32,
            TypeId::Builtin(BuiltinType::F64) => Self::F64,
            TypeId::Builtin(BuiltinType::Str) => Self::Str,
        }
    }
}

pub mod bytecode;
pub mod module;

use kagari_sema::TypedModule;
use smallvec::{SmallVec, smallvec};

pub use bytecode::Instruction;
pub use module::{
    FunctionBuffer, InstructionBuffer, IrFunction, IrModule, ParameterBuffer, ValueType,
};

pub fn lower_to_ir(module: &TypedModule) -> IrModule {
    let functions: FunctionBuffer = module
        .functions
        .iter()
        .map(|function| IrFunction {
            name: function.name.clone(),
            params: function
                .params
                .iter()
                .map(|param| module::IrParameter {
                    name: param.name.clone(),
                    ty: ValueType::from_type_id(param.ty),
                })
                .collect::<SmallVec<[_; 4]>>(),
            return_type: ValueType::from_type_id(function.return_type),
            code: smallvec![Instruction::Return],
        })
        .collect();

    IrModule { functions }
}

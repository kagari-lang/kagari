pub mod bytecode;
pub mod module;

use kagari_hir::typeck::TypedModule;
use smallvec::{SmallVec, smallvec};

pub fn lower_to_ir(module: &TypedModule) -> module::IrModule {
    let functions: module::FunctionBuffer = module
        .functions
        .iter()
        .map(|function| module::IrFunction {
            name: function.name.clone(),
            params: function
                .params
                .iter()
                .map(|param| module::IrParameter {
                    name: param.name.clone(),
                    ty: module::ValueType::from_type_id(param.ty),
                })
                .collect::<SmallVec<[_; 4]>>(),
            return_type: module::ValueType::from_type_id(function.return_type),
            code: smallvec![bytecode::Instruction::Return],
        })
        .collect();

    module::IrModule { functions }
}

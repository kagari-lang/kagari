pub mod bytecode;
pub mod module;

use kagari_sema::TypedModule;

pub use bytecode::Instruction;
pub use module::{IrFunction, IrModule, ValueType};

pub fn lower_to_ir(module: &TypedModule) -> IrModule {
    let functions = module
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
                .collect(),
            return_type: ValueType::from_type_id(function.return_type),
            code: vec![Instruction::Return],
        })
        .collect();

    IrModule { functions }
}

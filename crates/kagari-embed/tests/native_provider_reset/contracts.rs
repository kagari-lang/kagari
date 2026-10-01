use kagari_abi::{
    callable::{CallableImplementation, NativeBinding},
    provider::NativeContract,
    types::{FunctionAbi, PublicAbiItem},
};
use kagari_bytecode::KbcArtifact;

fn alter_function(function: &mut FunctionAbi, alter: &impl Fn(&mut NativeContract)) {
    if let CallableImplementation::Native(NativeBinding::Provider(contract)) =
        &mut function.implementation
    {
        alter(contract);
    }
}

pub(super) fn alter_contracts(artifact: &mut KbcArtifact, alter: impl Fn(&mut NativeContract)) {
    for module in &mut artifact.program.modules {
        for import in &mut module.native_imports {
            alter(&mut import.contract);
        }
        for declaration in &mut module.native_declarations {
            alter_function(&mut declaration.function, &alter);
        }
        for item in &mut module.public_items {
            match item {
                PublicAbiItem::Function(function) => alter_function(function, &alter),
                PublicAbiItem::Type(_) => {}
                PublicAbiItem::Trait(ty) => {
                    for method in &mut ty.methods {
                        alter_function(method, &alter);
                    }
                }
                PublicAbiItem::InterfaceTable(table) => {
                    for method in &mut table.methods {
                        alter_function(method, &alter);
                    }
                }
                PublicAbiItem::Const(_) => {}
            }
        }
    }
}

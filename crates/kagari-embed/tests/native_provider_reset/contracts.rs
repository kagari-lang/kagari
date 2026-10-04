use kagari_bytecode::artifact::KbcArtifact;
use kagari_common::identity::DefinitionPath;
use kagari_contract::types::PublicItem;
use kagari_types::{callable::CallableImplementation, declaration::FnDecl};

fn alter_function(function: &mut FnDecl, alter: &impl Fn(&mut DefinitionPath)) {
    if let CallableImplementation::Native(id) = &mut function.implementation {
        alter(id);
    }
}

pub(super) fn alter_bindings(artifact: &mut KbcArtifact, alter: impl Fn(&mut DefinitionPath)) {
    for module in &mut artifact.program.modules {
        for import in &mut module.native_imports {
            alter(&mut import.binding);
        }
        for declaration in &mut module.native_declarations {
            alter_function(&mut declaration.function, &alter);
        }
        for item in &mut module.public_items {
            match item {
                PublicItem::Function(function) => alter_function(function, &alter),
                PublicItem::Type(_) | PublicItem::Const(_) => {}
                PublicItem::Trait(ty) => {
                    for method in &mut ty.methods {
                        alter_function(method, &alter);
                    }
                }
                PublicItem::InterfaceTable(table) => {
                    for method in &mut table.methods {
                        alter_function(method, &alter);
                    }
                }
            }
        }
    }
}

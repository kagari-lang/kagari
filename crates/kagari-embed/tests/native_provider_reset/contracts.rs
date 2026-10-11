use kagari_bytecode::artifact::KbcArtifact;
use kagari_common::identity::{map::DefinitionContext, table::DefinitionId};
use kagari_contract::types::PublicItem;
use kagari_types::{callable::CallableImplementation, declaration::FnDecl};

fn alter_function(function: &mut FnDecl, alter: &impl Fn(&mut DefinitionId)) {
    if let CallableImplementation::Native(id) = &mut function.implementation {
        alter(id);
    }
}

pub(super) fn alter_bindings(artifact: &mut KbcArtifact, alter: impl Fn(&mut DefinitionId)) {
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
                PublicItem::InherentTable(table) => {
                    for method in &mut table.methods {
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

/// Forge a binding at its authoring boundary without changing the declaration it claims to implement.
pub(super) fn altered_binding(
    context: &DefinitionContext,
    id: DefinitionId,
    change: usize,
) -> DefinitionId {
    let definitions = context.snapshot();
    let view = definitions.resolve(id).unwrap();
    let mut owner = view.module().clone();
    if change == 0 {
        owner.package.0.push_str("-unknown");
    }
    let mut parent = context.intern_root(&owner).unwrap();
    let segments: Vec<_> = view.segments().collect();
    for (index, segment) in segments.iter().enumerate() {
        let name = if index + 1 == segments.len() && change != 0 {
            if change == 1 {
                format!("{}-unknown", segment.name)
            } else {
                "$foundation_list_new".to_owned()
            }
        } else {
            segment.name.to_owned()
        };
        parent = context
            .intern_child(parent, segment.kind, &name, segment.occurrence)
            .unwrap();
    }
    parent
}

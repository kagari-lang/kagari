use kagari_bytecode::{artifact::KbcArtifact, program::updates};
use kagari_common::identity::{map::DefinitionContext, table::DefinitionId};
use kagari_contract::types::{PublicItem, named::NamedContract};
use kagari_types::{callable::CallableImplementation, declaration::FnDecl};

fn alter_function(function: &mut FnDecl, alter: &impl Fn(&mut DefinitionId)) {
    if let CallableImplementation::Native(id) = &mut function.implementation {
        alter(id);
    }
}

pub(super) fn alter_bindings(
    artifact: &mut KbcArtifact,
    context: &DefinitionContext,
    alter: impl Fn(&mut DefinitionId),
) {
    for module in &mut artifact.program.modules {
        for import in &mut module.native_imports {
            alter(&mut import.binding);
        }
        for declaration in &mut module.native_declarations {
            alter_function(&mut declaration.function, &alter);
        }
        for record in &mut module.named_declarations {
            if let NamedContract::Function(function) = &mut record.contract {
                alter_function(function, &alter);
            }
        }
        for item in &mut module.public_items {
            match item {
                PublicItem::Function(function) => alter_function(function, &alter),
                // Static ABI records contain storage facts, not native callable bindings.
                PublicItem::Type(_) | PublicItem::Const(_) | PublicItem::Static(_) => {}
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
    let previous = &artifact.program.update_evidence;
    let mut evidence = updates::build(
        &artifact.program.modules,
        updates::EvidenceInputs {
            sources: previous.sources.clone(),
            initializers: previous.initializers.clone(),
            selections: previous.selections.clone(),
            classifications: previous
                .bodies
                .iter()
                .map(|body| (body.requirement.origin.clone(), body.classification.clone()))
                .collect(),
        },
        &context.snapshot(),
        &Default::default(),
    )
    .unwrap();
    evidence.source_manifest = previous.source_manifest.clone();
    artifact.program.update_evidence = evidence;
}

//! Extract shared construction demands from MIR's checked callable structure.
use crate::{
    function::{MirFunction, MirModule},
    instruction::{CallTarget, ConstructionTarget as MirConstruction, Instruction},
    program::construction::methods::{ConstructionModule, construction_methods},
};
use kagari_common::{cancellation::CancellationToken, identity::table::DefinitionTable};
use kagari_contract::{
    callable::construction::{
        ConstructionBody, ConstructionMethod, ConstructionStep, closed_constructions,
        native_construction,
    },
    decode_limits::MAX_TABLE_RECORDS,
    types::{ConcreteFunctionIdentity, PublicItem},
};
use kagari_types::ty::{NominalTy, Ty, substitution::TypeTransformError};
use std::{fmt, sync::Arc};

pub mod methods;

#[derive(Clone)]
pub(super) struct UpdateCoverage {
    pub methods: Arc<Vec<ConstructionMethod>>,
    pub constructions: Vec<ConcreteFunctionIdentity>,
    pub layouts: Vec<Ty>,
}

impl fmt::Debug for UpdateCoverage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UpdateCoverage")
            .field("method_count", &self.methods.len())
            .field("constructions", &self.constructions)
            .field("layouts", &self.layouts)
            .finish()
    }
}

/// Extract the construction-relevant graph of one completed body.
/// Closure/Future targets index `functions`, which must contain the drained local
/// planner bodies. Callers retain this summary instead of rescanning instructions.
pub fn function_construction(
    function: &MirFunction,
    functions: &[MirFunction],
    cancel: &CancellationToken,
) -> Result<ConstructionBody, TypeTransformError> {
    let mut steps = Vec::new();
    for instruction in function.blocks.iter().flat_map(|block| &block.instructions) {
        cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
        match instruction {
            Instruction::MakeInterface {
                target:
                    MirConstruction::Declared {
                        implementation,
                        arguments,
                        operations,
                        constructions,
                        ..
                    },
                ..
            } => {
                steps.push(ConstructionStep::Evidence {
                    operations: operations.clone(),
                    constructions: constructions.clone(),
                });
                steps.push(ConstructionStep::Interface(ConcreteFunctionIdentity {
                    declaration: *implementation,
                    arguments: arguments.clone(),
                }));
            }
            Instruction::Call {
                callee: CallTarget::InterfaceMethod(call),
                ..
            } => {
                steps.push(ConstructionStep::Evidence {
                    operations: call.operations.clone(),
                    constructions: call.constructions.clone(),
                });
                steps.push(ConstructionStep::Dispatch(call.as_ref().clone()));
            }
            Instruction::Call {
                callee: CallTarget::Shared(call),
                ..
            } => {
                steps.push(ConstructionStep::Evidence {
                    operations: call.operations.clone(),
                    constructions: call.constructions.clone(),
                });
                steps.push(ConstructionStep::Call {
                    body: call.instance.clone(),
                    arguments: call.arguments.clone(),
                });
            }
            Instruction::MakeClosure {
                function: target, ..
            }
            | Instruction::MakeFuture {
                function: target, ..
            } => {
                let target = functions
                    .get(target.index())
                    .ok_or(TypeTransformError::InvalidContract)?;
                steps.push(ConstructionStep::Call {
                    body: target.instance.clone(),
                    arguments: target
                        .semantic
                        .generic
                        .as_ref()
                        .map_or_else(Vec::new, |body| {
                            body.parameters.iter().map(|p| p.as_type()).collect()
                        }),
                });
            }
            _ => {}
        }
    }
    Ok(ConstructionBody {
        identity: function.instance.clone(),
        parameters: function
            .semantic
            .generic
            .as_ref()
            .map_or_else(Vec::new, |body| body.parameters.clone()),
        steps,
    })
}

fn collect(
    modules: &[&MirModule],
    definitions: &DefinitionTable,
    cancel: &CancellationToken,
) -> Result<(Vec<ConstructionBody>, Vec<ConstructionMethod>), TypeTransformError> {
    let mut bodies = Vec::new();
    for module in modules {
        cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
        for function in &module.functions {
            bodies.push(function_construction(function, &module.functions, cancel)?);
        }
    }
    for import in modules
        .iter()
        .flat_map(|module| module.native_applications())
    {
        if !bodies.iter().any(|body| body.identity == import.instance)
            && let Some(body) = native_construction(import)
        {
            bodies.push(body);
        }
    }
    let views = modules
        .iter()
        .map(|module| ConstructionModule::from(*module))
        .collect::<Vec<_>>();
    Ok((bodies, construction_methods(&views, definitions, cancel)?))
}

pub(super) fn verify_closed_interfaces(
    modules: &[&MirModule],
    definitions: &DefinitionTable,
    cancel: &CancellationToken,
) -> Result<UpdateCoverage, TypeTransformError> {
    let (bodies, methods) = collect(modules, definitions, cancel)?;
    for demand in closed_constructions(&bodies, &methods, MAX_TABLE_RECORDS, cancel)? {
        let Some((owner, _)) = modules.iter().find_map(|module| {
            module.abi.public_items.iter().find_map(|item| match item {
                PublicItem::InterfaceTable(table) if table.declaration == demand.declaration => {
                    table
                        .instantiate(&demand.arguments, &[], definitions)
                        .map(|table| (module, table))
                }
                _ => None,
            })
        }) else {
            return Err(TypeTransformError::InvalidContract);
        };
        if !owner.interface_instances.contains(&demand) {
            return Err(TypeTransformError::InvalidContract);
        }
    }
    let mut constructions = Vec::new();
    let mut layouts = Vec::new();
    for module in modules {
        cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
        constructions.extend(
            module
                .interface_instances
                .iter()
                .filter(|instance| instance.arguments.iter().all(Ty::is_concrete))
                .cloned(),
        );
        for layout in &module.structures {
            cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
            if layout.arguments.iter().all(Ty::is_concrete) {
                layouts.push(Ty::Struct(NominalTy {
                    declaration: layout.declaration,
                    arguments: layout.arguments.clone(),
                    associated_types: Default::default(),
                }));
            }
        }
        for layout in &module.enumerations {
            cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
            if layout.arguments.iter().all(Ty::is_concrete) {
                layouts.push(Ty::Enum(NominalTy {
                    declaration: layout.declaration,
                    arguments: layout.arguments.clone(),
                    associated_types: Default::default(),
                }));
            }
        }
    }
    Ok(UpdateCoverage {
        methods: Arc::new(methods),
        constructions,
        layouts,
    })
}

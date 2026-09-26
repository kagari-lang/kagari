//! Compile checked host mappings into ordinary verified interface call bridges.
use super::{IrLoweringError, instances::InstancePlanner};
use crate::module::{
    ModuleAbi, PublicAbiItem,
    abi::{AbiType, FunctionAbi, InterfaceTableAbi, NominalAbiType, ParameterAbi},
    function::{
        BasicBlock, FunctionInstance, IrFunction, IrFunctionDebugMetadata, IrLexicalScope, IrLocal,
        IrLocalDebugInfo, IrParameter, IrTemp,
    },
    ids::{BlockId, InstanceId, LocalId, TempId},
    instruction::{CallTarget, Instruction, IrValue, Terminator},
};
use kagari_common::identity::{DefinitionKind, DefinitionPathSegment};

pub(super) fn collect(
    planner: &mut InstancePlanner<'_>,
    module: &kagari_hir::AnalyzedModule,
    abi: &mut ModuleAbi,
    functions: &mut Vec<IrFunction>,
) -> Result<(), IrLoweringError> {
    for (declaration, receiver, interface, span) in planner.host_interfaces.clone() {
        planner.check()?;
        let host_id = match &receiver {
            kagari_hir::types::TypeId::Host(id) => id,
            _ => return Err(IrLoweringError::MissingBinding("host interface receiver")),
        };
        let host = module
            .names
            .hosts
            .nominal_type(host_id)
            .and_then(|id| module.names.hosts.type_declaration(id))
            .ok_or(IrLoweringError::MissingBinding(
                "host interface declaration",
            ))?;
        let implementation = module
            .names
            .hosts
            .interface_implementation(&interface, &receiver)
            .ok_or(IrLoweringError::MissingBinding(
                "checked host interface mapping",
            ))?;
        let contract = module
            .aggregates
            .trait_(&interface.declaration)
            .ok_or(IrLoweringError::MissingBinding("host interface trait"))?;
        let mut methods = Vec::new();
        for method in &contract.methods {
            planner.check()?;
            let binding = implementation
                .methods
                .iter()
                .find(|binding| binding.trait_method == method.id)
                .ok_or(IrLoweringError::MissingBinding("host interface method"))?;
            let host_call = host
                .method_contract(&binding.host_method)
                .map_err(|_| IrLoweringError::MissingBinding("host method contract"))?;
            let params: Vec<_> = host_call
                .params
                .iter()
                .map(|param| ParameterAbi {
                    name: param.name.clone(),
                    mutable: false,
                    ty: AbiType::from_host_type(&param.ty),
                })
                .collect();
            let result_type = AbiType::from_host_type(&host_call.return_type);
            methods.push(FunctionAbi {
                name: method.name.clone(),
                generic_params: Vec::new(),
                bounds: Vec::new(),
                params: params.clone(),
                return_type: result_type.clone(),
            });
            let mut method_id = declaration.clone();
            method_id.path.push(DefinitionPathSegment {
                kind: DefinitionKind::Method,
                name: method.name.clone(),
                occurrence: 0,
            });
            let mut instructions = Vec::new();
            let mut temps = Vec::new();
            let mut arguments = Vec::new();
            for (index, param) in params.iter().enumerate() {
                let value = IrValue {
                    temp: TempId::new(index),
                    ty: param.ty.representation(),
                };
                temps.push(IrTemp { ty: value.ty });
                instructions.push(Instruction::LoadLocal {
                    dst: value,
                    local: LocalId::new(index),
                });
                arguments.push(value);
            }
            let result = IrValue {
                temp: TempId::new(temps.len()),
                ty: result_type.representation(),
            };
            temps.push(IrTemp { ty: result.ty });
            instructions.push(Instruction::Call {
                dst: Some(result),
                callee: CallTarget::HostFunction(Box::new(host_call)),
                args: arguments.into(),
            });
            let terminator = Terminator::Return(Some(result));
            let count = instructions.len();
            let effects = instructions
                .iter()
                .fold(terminator.effects(), |effects, instruction| {
                    effects.union(instruction.effects())
                });
            for _ in 0..=count {
                planner.charge_instruction(span);
            }
            planner.check()?;
            functions.push(IrFunction {
                id: InstanceId::new(functions.len()),
                instance: FunctionInstance {
                    declaration: method_id,
                    arguments: Vec::new(),
                },
                name: format!(
                    "$host-interface#{}::{}",
                    declaration.path[0].occurrence, method.name
                ),
                params: params
                    .iter()
                    .enumerate()
                    .map(|(index, param)| IrParameter {
                        name: param.name.clone(),
                        ty: param.ty.representation(),
                        local: LocalId::new(index),
                    })
                    .collect(),
                return_type: result.ty,
                locals: params
                    .iter()
                    .map(|param| IrLocal {
                        name: param.name.clone(),
                        ty: param.ty.representation(),
                    })
                    .collect(),
                temps,
                blocks: vec![BasicBlock {
                    instructions,
                    instruction_spans: vec![span; count],
                    instruction_scopes: vec![0; count],
                    terminator: Some(terminator),
                    terminator_span: Some(span),
                    terminator_scope: Some(0),
                }],
                entry: BlockId::new(0),
                effects,
                debug: IrFunctionDebugMetadata {
                    source: Some(module.lowered.source.clone()),
                    source_module: Some(module.lowered.source.module_identity().clone()),
                    source_span: span,
                    locals: params
                        .iter()
                        .enumerate()
                        .map(|(index, param)| IrLocalDebugInfo {
                            local: LocalId::new(index),
                            name: param.name.clone(),
                            span,
                            ty: param.ty.representation(),
                            is_parameter: true,
                        })
                        .collect(),
                    captured_bindings: Vec::new(),
                    lexical_scopes: vec![IrLexicalScope {
                        parent: None,
                        local: None,
                    }],
                },
            });
        }
        abi.public_items
            .push(PublicAbiItem::InterfaceTable(Box::new(InterfaceTableAbi {
                associated_type_families: Vec::new(),
                associated_consts: Vec::new(),
                host_bridge: true,
                declaration,
                name: format!(
                    "{} as {}",
                    host.symbol,
                    interface.declaration.path.last().unwrap().name
                ),
                generic_params: Vec::new(),
                bounds: Vec::new(),
                trait_type: AbiType::Trait(NominalAbiType::from_checked_type(&interface)),
                for_type: AbiType::from_checked_type(&receiver),
                methods,
            })));
    }
    Ok(())
}

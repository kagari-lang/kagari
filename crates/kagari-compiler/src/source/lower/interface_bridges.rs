//! Compile checked host mappings and intrinsic protocol evidence into ordinary verified interface call bridges.

use crate::source::lower::{
    MirLoweringError,
    debug::capture_origin,
    instances::{InstancePlanner, InterfaceBridgeRequest},
};
use kagari_common::identity::{
    DefinitionKind, mapping::DefinitionMappingError, table::DefinitionId,
};
use kagari_contract::{
    native_import::NativeImport,
    representation::semantic_representation,
    slots::SemanticSlots,
    types::{
        ConcreteFunctionIdentity, InterfaceBridge, InterfaceTable, ModuleContract, PublicItem,
    },
};
use kagari_hir::{AnalyzedModule, types::TypeId};
use kagari_mir::{
    debug::{MirFunctionDebugMetadata, MirLexicalScope, MirLocalDebugInfo},
    function::{BasicBlock, MirFunction, MirLocal, MirParameter, MirTemp},
    ids::{BlockId, InstanceId, LocalId, TempId},
    instruction::{CallTarget, Instruction, MirValue, Terminator},
};
use kagari_types::{
    callable::CallableImplementation,
    declaration::{FnDecl, Param},
    language::Protocol,
    ty::Ty,
};

pub(super) fn collect(
    planner: &mut InstancePlanner<'_>,
    module: &AnalyzedModule,
    abi: &mut ModuleContract,
    functions: &mut Vec<MirFunction>,
) -> Result<(), MirLoweringError> {
    for request in planner.interface_bridges.clone() {
        planner.check()?;
        let InterfaceBridgeRequest {
            declaration,
            ref receiver,
            ref interface,
            span,
            kind,
        } = request;
        let contract = module
            .aggregates
            .trait_(&interface.declaration)
            .ok_or(MirLoweringError::MissingBinding("interface bridge trait"))?;
        let mut methods = Vec::new();
        for method in &contract.methods {
            planner.check()?;
            let (params, result_type, callee) =
                resolve_target(planner, module, functions, &request, method.id)?;
            methods.push(FnDecl {
                method_policy: Default::default(),
                implementation: CallableImplementation::Script,
                name: method.name.clone(),
                generic_params: Vec::new(),
                bounds: Vec::new(),
                params: params.clone(),
                return_type: result_type.clone(),
            });
            let method_id = planner
                .context
                .intern_child(declaration, DefinitionKind::Method, &method.name, 0)
                .map_err(DefinitionMappingError::from)?;
            let mut instructions = Vec::new();
            let mut temps = Vec::new();
            let mut arguments = Vec::new();
            for (index, param) in params.iter().enumerate() {
                let value = MirValue {
                    temp: TempId::new(index),
                    ty: semantic_representation(&param.ty),
                };
                temps.push(MirTemp { ty: value.ty });
                instructions.push(Instruction::LoadLocal {
                    dst: value,
                    local: LocalId::new(index),
                });
                arguments.push(value);
            }
            let result = MirValue {
                temp: TempId::new(temps.len()),
                ty: semantic_representation(&result_type),
            };
            temps.push(MirTemp { ty: result.ty });
            instructions.push(Instruction::Call {
                dst: Some(result),
                callee,
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
            let mut function = MirFunction {
                semantic: SemanticSlots {
                    generic: None,
                    protocol_adapter: None,
                    params: params
                        .iter()
                        .enumerate()
                        .map(|(i, p)| (i, p.ty.clone()))
                        .collect(),
                    locals: params
                        .iter()
                        .enumerate()
                        .map(|(i, p)| (i, p.ty.clone()))
                        .collect(),
                    result: Some(result_type.clone()),
                    registers: Default::default(),
                },
                id: InstanceId::new(functions.len()),
                instance: ConcreteFunctionIdentity {
                    declaration: method_id,
                    arguments: Vec::new(),
                },
                name: format!(
                    "$host-interface#{}::{}",
                    planner
                        .definitions()
                        .resolve(declaration)
                        .expect("host bridge identity")
                        .last()
                        .expect("host bridge member")
                        .occurrence,
                    method.name
                ),
                params: params
                    .iter()
                    .enumerate()
                    .map(|(index, param)| MirParameter {
                        name: param.name.clone(),
                        ty: semantic_representation(&param.ty),
                        local: LocalId::new(index),
                    })
                    .collect(),
                return_type: result.ty,
                locals: params
                    .iter()
                    .map(|param| MirLocal {
                        name: param.name.clone(),
                        ty: semantic_representation(&param.ty),
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
                debug: MirFunctionDebugMetadata {
                    source: None,
                    source_module: Some(module.lowered.source.module_identity().clone()),
                    source_span: span,
                    locals: params
                        .iter()
                        .enumerate()
                        .map(|(index, param)| MirLocalDebugInfo {
                            local: LocalId::new(index),
                            name: param.name.clone(),
                            span,
                            ty: semantic_representation(&param.ty),
                            is_parameter: true,
                        })
                        .collect(),
                    captured_bindings: Vec::new(),
                    lexical_scopes: vec![MirLexicalScope {
                        parent: None,
                        local: None,
                    }],
                },
            };
            function.debug.source = Some(capture_origin(
                &function,
                &module.lowered.source,
                &planner.options.cancel,
            )?);
            functions.push(function);
        }
        abi.public_items
            .push(PublicItem::InterfaceTable(Box::new(InterfaceTable {
                associated_type_families: Vec::new(),
                associated_consts: Vec::new(),
                bridge: Some(kind),
                declaration,
                name: format!(
                    "{} as {}",
                    receiver.display_name(&planner.definitions()),
                    contract.declaration.name
                ),
                generic_params: Vec::new(),
                bounds: Vec::new(),
                trait_type: Ty::Trait(planner.encode_nominal(interface)),
                for_type: planner.encode_type(receiver),
                methods,
            })));
    }
    Ok(())
}

fn resolve_target(
    planner: &mut InstancePlanner<'_>,
    module: &AnalyzedModule,
    functions: &[MirFunction],
    request: &InterfaceBridgeRequest,
    method: DefinitionId,
) -> Result<(Vec<Param>, Ty, CallTarget), MirLoweringError> {
    let receiver = &request.receiver;
    let interface = &request.interface;
    match request.kind {
        InterfaceBridge::Host => {
            let TypeId::Host(host_id) = &receiver else {
                return Err(MirLoweringError::MissingBinding("host interface receiver"));
            };
            let host = module
                .names
                .hosts
                .nominal_type(host_id)
                .and_then(|id| module.names.hosts.type_declaration(id))
                .ok_or(MirLoweringError::MissingBinding(
                    "host interface declaration",
                ))?;
            let implementation = module
                .names
                .hosts
                .interface_implementation(interface, receiver)
                .ok_or(MirLoweringError::MissingBinding(
                    "checked host interface mapping",
                ))?;
            let binding = implementation
                .methods
                .iter()
                .find(|binding| binding.trait_method == method)
                .ok_or(MirLoweringError::MissingBinding("host interface method"))?;
            let call = host
                .method_contract(&binding.host_method, &planner.definitions())
                .map_err(|_| MirLoweringError::MissingBinding("host method contract"))?;
            let params = call
                .params
                .iter()
                .map(|param| Param {
                    name: param.name.clone(),
                    mutable: false,
                    ty: Ty::from_host_type(&param.ty),
                })
                .collect::<Vec<_>>();
            Ok((
                params,
                Ty::from_host_type(&call.return_type),
                CallTarget::Native(Box::new(NativeImport::from_host(&call))),
            ))
        }
        InterfaceBridge::Intrinsic => {
            let protocol =
                Protocol::from_definition(&interface.declaration, &planner.definitions()).ok_or(
                    MirLoweringError::MissingBinding("intrinsic interface protocol"),
                )?;
            let target = planner.enqueue_selected_protocol(
                protocol,
                receiver,
                interface,
                request.span,
                false,
            )?;
            let function =
                functions
                    .get(target.index())
                    .ok_or(MirLoweringError::MissingBinding(
                        "prepared intrinsic interface method",
                    ))?;
            let params = function
                .semantic
                .params
                .values()
                .enumerate()
                .map(|(index, ty)| Param {
                    name: if index == 0 {
                        "self".into()
                    } else {
                        format!("arg_{index}")
                    },
                    mutable: false,
                    ty: ty.clone(),
                })
                .collect();
            Ok((
                params,
                function
                    .semantic
                    .result
                    .clone()
                    .ok_or(MirLoweringError::MissingBinding("intrinsic result"))?,
                CallTarget::Function(target),
            ))
        }
    }
}

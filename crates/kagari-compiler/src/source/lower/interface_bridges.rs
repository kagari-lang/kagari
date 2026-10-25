//! Compile intrinsic protocol evidence into ordinary verified interface call bridges.

use crate::source::lower::{
    MirLoweringError,
    debug::capture_origin,
    instances::{InstancePlanner, InterfaceBridgeRequest},
};
use kagari_common::identity::{
    DefinitionKind, mapping::DefinitionMappingError, table::DefinitionId,
};
use kagari_contract::{
    callable::generic::GenericBody,
    representation::semantic_representation,
    slots::SemanticSlots,
    types::{
        ConcreteFunctionIdentity, InterfaceBridge, InterfaceTable, ModuleContract, PublicItem,
    },
    updates::BodyClassification,
};
use kagari_hir::{AnalyzedModule, types::semantic::raise_type};
use kagari_mir::{
    debug::{MirFunctionDebugMetadata, MirLexicalScope, MirLocalDebugInfo},
    function::{BasicBlock, MirFunction, MirLocal, MirParameter, MirTemp},
    ids::{BlockId, InstanceId, LocalId, TempId},
    instruction::{CallTarget, Instruction, MirValue, Terminator},
};
use kagari_types::{
    callable::CallableImplementation,
    declaration::{FnDecl, Param, requirement::NativeCallableRequirement},
    language::{Protocol, adapter::adapter_contract},
    ty::{
        GenericBound, GenericParam, NominalTy, Ty,
        substitution::{TypeSubstitution, TypeTransformError},
    },
};

/// Wrappers are not planner slots. Their IDs are tail-relative until a snapshot.
#[derive(Default)]
pub(super) struct InterfaceBridges {
    pub functions: Vec<MirFunction>,
    pub processed: usize,
}

pub(super) struct PreparedBridge {
    request: InterfaceBridgeRequest,
    methods: Vec<PreparedMethod>,
}

struct PreparedMethod {
    declaration: DefinitionId,
    name: String,
    target: PreparedTarget,
}

enum PreparedTarget {
    Function(InstanceId),
    Shared(Box<MethodTarget>),
}

struct MethodTarget {
    params: Vec<Param>,
    result_type: Ty,
    callee: CallTarget,
    generic: Option<GenericBody>,
}

impl InterfaceBridges {
    /// Prepare dependencies before the caller drains newly admitted planner bodies.
    pub fn prepare(
        &self,
        planner: &mut InstancePlanner<'_>,
        module: &AnalyzedModule,
    ) -> Result<Option<PreparedBridge>, MirLoweringError> {
        let Some(request) = planner.interface_bridges.get(self.processed).cloned() else {
            return Ok(None);
        };
        let contract = module
            .aggregates
            .trait_(&request.interface.declaration)
            .ok_or(MirLoweringError::MissingBinding("interface bridge trait"))?;
        let mut methods = Vec::new();
        for method in &contract.methods {
            planner.check()?;
            let declaration = planner
                .context
                .intern_child(request.declaration, DefinitionKind::Method, &method.name, 0)
                .map_err(DefinitionMappingError::from)?;
            methods.push(PreparedMethod {
                declaration,
                name: method.name.clone(),
                target: prepare_target(planner, &request, declaration)?,
            });
        }
        Ok(Some(PreparedBridge { request, methods }))
    }

    pub fn emit(
        &mut self,
        planner: &mut InstancePlanner<'_>,
        module: &AnalyzedModule,
        abi: &mut ModuleContract,
        functions: &[MirFunction],
        classifications: &mut Vec<(ConcreteFunctionIdentity, BodyClassification)>,
        prepared: PreparedBridge,
    ) -> Result<(), MirLoweringError> {
        planner.check()?;
        let PreparedBridge {
            request,
            methods: prepared_methods,
        } = prepared;
        let InterfaceBridgeRequest {
            declaration,
            ref parameters,
            ref receiver,
            ref interface,
            span,
        } = request;
        let contract = module
            .aggregates
            .trait_(&interface.declaration)
            .ok_or(MirLoweringError::MissingBinding("interface bridge trait"))?;
        let mut methods = Vec::new();
        for method in prepared_methods {
            planner.check()?;
            let method_id = method.declaration;
            let MethodTarget {
                params,
                result_type,
                callee,
                generic,
            } = resolve_target(method.target, functions)?;
            let mut table_bindings = TypeSubstitution::default();
            let table_arguments: Vec<_> = parameters.iter().map(GenericParam::as_type).collect();
            for (parameter, argument) in parameters.iter().zip(&table_arguments) {
                table_bindings.bind(&method_id, parameter.position, argument);
            }
            methods.push(FnDecl {
                host_policy: None,
                method_policy: Default::default(),
                implementation: CallableImplementation::Script,
                name: method.name.clone(),
                generic_params: Vec::new(),
                bounds: Vec::new(),
                params: params
                    .iter()
                    .map(|param| {
                        Ok(Param {
                            ty: table_bindings.apply(&param.ty, &planner.options.cancel)?,
                            ..param.clone()
                        })
                    })
                    .collect::<Result<_, TypeTransformError>>()
                    .map_err(|_| MirLoweringError::MissingBinding("tuple bridge signature"))?,
                return_type: result_type.clone(),
            });
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
                    generic,
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
                id: InstanceId::new(self.functions.len()),
                instance: ConcreteFunctionIdentity {
                    declaration: method_id,
                    arguments: Vec::new(),
                },
                name: format!(
                    "$intrinsic-interface#{}::{}",
                    planner
                        .definitions()
                        .resolve(declaration)
                        .expect("intrinsic bridge identity")
                        .last()
                        .expect("intrinsic bridge member")
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
            classifications.push((
                function.instance.clone(),
                BodyClassification::IntrinsicAdapter(function.instance.clone()),
            ));
            #[cfg(test)]
            crate::source::work_metrics::body();
            self.functions.push(function);
        }
        abi.public_items
            .push(PublicItem::InterfaceTable(Box::new(InterfaceTable {
                associated_type_families: Vec::new(),
                associated_consts: Vec::new(),
                bridge: Some(InterfaceBridge::Intrinsic),
                declaration,
                name: format!(
                    "{} as {}",
                    receiver.display_name(&planner.definitions()),
                    contract.declaration.name
                ),
                generic_params: parameters.clone(),
                bounds: bridge_bounds(parameters, &planner.encode_nominal(interface)),
                trait_type: Ty::Trait(planner.encode_nominal(interface)),
                for_type: planner.encode_type(receiver),
                methods,
            })));
        self.processed += 1;
        Ok(())
    }
}

fn prepare_target(
    planner: &mut InstancePlanner<'_>,
    request: &InterfaceBridgeRequest,
    method: DefinitionId,
) -> Result<PreparedTarget, MirLoweringError> {
    let receiver = &request.receiver;
    let interface = &request.interface;
    let protocol = Protocol::from_definition(&interface.declaration, &planner.definitions())
        .ok_or(MirLoweringError::MissingBinding(
            "intrinsic interface protocol",
        ))?;
    if !request.parameters.is_empty() {
        let parameters: Vec<_> = request
            .parameters
            .iter()
            .map(|parameter| GenericParam {
                owner: method,
                position: parameter.position,
            })
            .collect();
        let interface = planner.encode_nominal(interface);
        let receiver = Ty::Tuple(parameters.iter().map(GenericParam::as_type).collect());
        let member = planner
            .definitions()
            .lookup_child(
                interface.declaration,
                DefinitionKind::Method,
                if protocol == Protocol::Hash {
                    "hash"
                } else {
                    "eq"
                },
                0,
            )
            .ok_or(MirLoweringError::MissingBinding("tuple bridge member"))?;
        let required = NativeCallableRequirement {
            receiver: receiver.clone(),
            interface: interface.clone(),
            member,
            arguments: vec![],
        };
        let (_, signature) = adapter_contract(&required, &planner.definitions())
            .ok_or(MirLoweringError::MissingBinding("tuple bridge signature"))?;
        let target = planner.enqueue_selected_protocol(
            protocol,
            &raise_type(&receiver),
            &request.interface,
            request.span,
            true,
        )?;
        let call = planner.tuple_protocol_call(target, required, request.span)?;
        let params = signature
            .params
            .into_iter()
            .enumerate()
            .map(|(index, ty)| Param {
                name: if index == 0 {
                    "self".into()
                } else {
                    format!("arg_{index}")
                },
                mutable: false,
                ty,
            })
            .collect();
        return Ok(PreparedTarget::Shared(Box::new(MethodTarget {
            params,
            result_type: signature.result,
            callee: CallTarget::Shared(Box::new(call)),
            generic: Some(GenericBody {
                bounds: bridge_bounds(&parameters, &interface),
                parameters,
            }),
        })));
    }
    let target =
        planner.enqueue_selected_protocol(protocol, receiver, interface, request.span, false)?;
    Ok(PreparedTarget::Function(target))
}

fn resolve_target(
    target: PreparedTarget,
    functions: &[MirFunction],
) -> Result<MethodTarget, MirLoweringError> {
    let target = match target {
        PreparedTarget::Shared(target) => return Ok(*target),
        PreparedTarget::Function(target) => target,
    };
    let function = functions
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
    Ok(MethodTarget {
        params,
        result_type: function
            .semantic
            .result
            .clone()
            .ok_or(MirLoweringError::MissingBinding("intrinsic result"))?,
        callee: CallTarget::Function(target),
        generic: None,
    })
}

fn bridge_bounds(parameters: &[GenericParam], interface: &NominalTy) -> Vec<GenericBound> {
    parameters
        .iter()
        .map(|parameter| GenericBound {
            ty: parameter.as_type(),
            constraints: vec![interface.clone()],
        })
        .collect()
}

//! Executable interface slots use checked declaration applications, without bodies
//! or per-library method selection for registered native entries.
use crate::bytecode::{BytecodeLoweringError, defaults};
use kagari_abi::{
    callable::CallableImplementation,
    ids::FunctionRef,
    native_import::NativeImport,
    types::{AbiType, ConcreteFunctionIdentity, GenericParameterAbi, PublicAbiItem},
};
use kagari_bytecode::{
    instruction::NativeImportId,
    module::{CallableTarget, InterfaceMethodSlot, InterfaceTableRecord},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionKind, DefinitionPath, DefinitionPathSegment},
};
use kagari_mir::{
    instruction::Instruction, program::VerifiedMirProgram, verify::VerifiedMirModule,
};
use std::slice;

pub(super) fn interface_instances(
    ir: &VerifiedMirModule,
    program: Option<&VerifiedMirProgram>,
) -> Vec<ConcreteFunctionIdentity> {
    let mut instances = ir
        .abi
        .public_items
        .iter()
        .filter_map(|item| {
            let PublicAbiItem::InterfaceTable(table) = item else {
                return None;
            };
            Some(ConcreteFunctionIdentity {
                declaration: table.declaration.clone(),
                arguments: vec![],
            })
        })
        .collect::<Vec<_>>();
    let owners = program
        .map(VerifiedMirProgram::modules)
        .unwrap_or(slice::from_ref(ir));
    let allocations = owners
        .iter()
        .flat_map(|owner| &owner.functions)
        .flat_map(|function| &function.blocks)
        .flat_map(|block| &block.instructions)
        .filter_map(|instruction| match instruction {
            Instruction::MakeInterface {
                implementation,
                arguments,
                ..
            } => Some(ConcreteFunctionIdentity {
                declaration: implementation.clone(),
                arguments: arguments.clone(),
            }),
            _ => None,
        });
    let demands = owners
        .iter()
        .flat_map(|owner| owner.interface_instances.iter().cloned());
    for mut instance in allocations.chain(demands) {
        if instance.declaration.module != ir.identity {
            continue;
        }
        instance.arguments = table_arguments(ir, &instance.declaration, &instance.arguments);
        if !instances.contains(&instance) {
            instances.push(instance);
        }
    }
    instances
}

pub(super) fn table_arguments(
    ir: &VerifiedMirModule,
    declaration: &DefinitionPath,
    arguments: &[AbiType],
) -> Vec<AbiType> {
    if arguments.iter().all(AbiType::is_concrete) {
        return arguments.to_vec();
    }
    ir.abi
        .public_items
        .iter()
        .find_map(|item| match item {
            PublicAbiItem::InterfaceTable(table) if table.declaration == *declaration => {
                Some(table.generic_params.iter().map(|p| p.as_type()).collect())
            }
            _ => None,
        })
        .expect("verified interface template")
}

pub(super) fn collect_interface_tables(
    ir: &VerifiedMirModule,
    program: Option<&VerifiedMirProgram>,
    imports: &mut Vec<NativeImport>,
) -> Result<Vec<InterfaceTableRecord>, BytecodeLoweringError> {
    let closure = program
        .map(VerifiedMirProgram::modules)
        .unwrap_or(slice::from_ref(ir));
    let cancel = CancellationToken::default();
    let has_defaults = closure
        .iter()
        .flat_map(|module| &module.abi.public_items)
        .filter_map(|item| match item {
            PublicAbiItem::InterfaceTable(table) => Some(table),
            _ => None,
        })
        .flat_map(|table| &table.methods)
        .any(|method| {
            matches!(
                method.implementation,
                CallableImplementation::NativeDefault(_)
            )
        });
    let catalog = if has_defaults {
        Some(defaults::catalog(
            &closure.iter().collect::<Vec<_>>(),
            &cancel,
        )?)
    } else {
        None
    };
    interface_instances(ir, program)
        .into_iter()
        .map(|instance| {
            let abi = ir
                .abi
                .public_items
                .iter()
                .find_map(|item| match item {
                    PublicAbiItem::InterfaceTable(table)
                        if table.declaration == instance.declaration =>
                    {
                        Some(table.as_ref())
                    }
                    _ => None,
                })
                .expect("verified interface template");
            let AbiType::Trait(interface) = &abi.trait_type else {
                unreachable!("verified interface type");
            };
            let mut methods = vec![];
            let shared_receiver = instance.arguments.iter().any(|ty| !ty.is_concrete());
            for method in &abi.methods {
                if instance.arguments.is_empty() && !abi.generic_params.is_empty() {
                    continue;
                }
                let conditional =
                    matches!(method.implementation, CallableImplementation::Native(_))
                        && method.bounds.iter().any(|bound| {
                            bound.constraints.iter().any(|constraint| {
                                !abi.bounds.iter().any(|assumed| {
                                    assumed.ty == bound.ty
                                        && assumed.constraints.contains(constraint)
                                })
                            })
                        });
                let shared_receiver =
                    shared_receiver || (conditional && !abi.generic_params.is_empty());
                let segment = DefinitionPathSegment {
                    kind: DefinitionKind::Method,
                    name: method.name.clone(),
                    occurrence: 0,
                };
                let declaration = child(&abi.declaration, segment.clone());
                let member = child(&interface.declaration, segment);
                let mut entry_arguments: Vec<_> = if shared_receiver {
                    instance
                        .arguments
                        .iter()
                        .cloned()
                        .chain(method.generic_params.iter().map(|p| p.as_type()))
                        .collect()
                } else {
                    method.generic_params.iter().map(|p| p.as_type()).collect()
                };
                if matches!(
                    method.implementation,
                    CallableImplementation::Native(_) | CallableImplementation::NativeDefault(_)
                ) {
                    if !method.generic_params.is_empty()
                        && method
                            .params
                            .first()
                            .is_none_or(|parameter| parameter.name != "self")
                    {
                        continue;
                    }
                    let target = if matches!(
                        method.implementation,
                        CallableImplementation::NativeDefault(_)
                    ) {
                        let applied = abi
                            .instantiate_in(&instance.arguments, &abi.generic_params)
                            .ok_or(BytecodeLoweringError::InvalidNativeInterface)?;
                        let applied = applied
                            .methods
                            .iter()
                            .find(|candidate| candidate.name == method.name)
                            .ok_or(BytecodeLoweringError::InvalidNativeInterface)?;
                        let CallableImplementation::NativeDefault(application) =
                            &applied.implementation
                        else {
                            unreachable!("applied default");
                        };
                        let mut assumptions = abi
                            .instantiate_in(&instance.arguments, &abi.generic_params)
                            .ok_or(BytecodeLoweringError::InvalidNativeInterface)?
                            .bounds;
                        assumptions.extend(applied.bounds.clone());
                        let scope = abi
                            .generic_params
                            .iter()
                            .chain(&applied.generic_params)
                            .cloned()
                            .collect::<Vec<_>>();
                        let mut instance = catalog
                            .as_ref()
                            .expect("default catalog")
                            .resolve_native_default_in(application, &scope, &assumptions, &cancel)
                            .map_err(|_| BytecodeLoweringError::InvalidNativeInterface)?
                            .ok_or(BytecodeLoweringError::InvalidNativeInterface)?
                            .instance;
                        if !instance.arguments.is_empty() {
                            entry_arguments = instance.arguments.clone();
                            instance.arguments = (0..instance.arguments.len())
                                .map(|position| {
                                    GenericParameterAbi {
                                        owner: instance.declaration.clone(),
                                        position,
                                    }
                                    .as_type()
                                })
                                .collect();
                        } else {
                            entry_arguments.clear();
                        }
                        instance
                    } else {
                        ConcreteFunctionIdentity {
                            arguments: if shared_receiver {
                                entry_arguments
                                    .iter()
                                    .enumerate()
                                    .map(|(position, _)| {
                                        GenericParameterAbi {
                                            owner: declaration.clone(),
                                            position,
                                        }
                                        .as_type()
                                    })
                                    .collect()
                            } else {
                                instance
                                    .arguments
                                    .iter()
                                    .cloned()
                                    .chain(
                                        method
                                            .generic_params
                                            .iter()
                                            .map(|parameter| parameter.as_type()),
                                    )
                                    .collect()
                            },
                            declaration,
                        }
                    };
                    let contract = ir
                        .native_targets
                        .iter()
                        .find(|contract| contract.instance == target)
                        .cloned()
                        .ok_or(BytecodeLoweringError::InvalidNativeInterface)?;
                    let index = imports
                        .iter()
                        .position(|existing| *existing == contract)
                        .unwrap_or_else(|| {
                            let index = imports.len();
                            imports.push(contract);
                            index
                        });
                    methods.push(InterfaceMethodSlot {
                        method: member,
                        target: CallableTarget::Native(NativeImportId::new(index)),
                        arguments: entry_arguments,
                    });
                } else {
                    for function in &ir.functions {
                        if function.instance.declaration == declaration
                            && function.instance.arguments
                                == if shared_receiver {
                                    vec![]
                                } else {
                                    instance.arguments.clone()
                                }
                        {
                            methods.push(InterfaceMethodSlot {
                                method: member.clone(),
                                target: CallableTarget::Script(FunctionRef::new(
                                    function.id.index(),
                                )),
                                arguments: entry_arguments.clone(),
                            });
                        }
                    }
                }
            }
            Ok(InterfaceTableRecord {
                parents: vec![],
                view: None,
                declaration: instance.declaration,
                arguments: instance.arguments,
                methods,
            })
        })
        .collect()
}

fn child(owner: &DefinitionPath, segment: DefinitionPathSegment) -> DefinitionPath {
    let mut declaration = owner.clone();
    declaration.path.push(segment);
    declaration
}

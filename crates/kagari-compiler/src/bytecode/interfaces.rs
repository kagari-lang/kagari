//! Executable interface slots use checked declaration applications, without bodies
//! or per-library method selection for registered native entries.
use crate::bytecode::{BytecodeLoweringError, defaults::Contracts};
use kagari_bytecode::{
    instruction::NativeImportId,
    module::{CallableTarget, InterfaceMethodSlot, InterfaceTableRecord},
};
use kagari_common::identity::table::DefinitionId;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionKind, DefinitionPathSegment},
};
use kagari_contract::{
    callable::CallableImplementation,
    ids::FunctionRef,
    native_import::NativeImport,
    types::{ConcreteFunctionIdentity, GenericParam, PublicItem, Ty},
};
use kagari_mir::{
    instruction::Instruction, program::VerifiedMirProgram, verify::VerifiedMirModule,
};
use std::slice;

pub(super) fn interface_instances(
    ir: &VerifiedMirModule,
    program: Option<&VerifiedMirProgram>,
) -> Vec<ConcreteFunctionIdentity<DefinitionId>> {
    let mut instances = ir
        .abi
        .public_items
        .iter()
        .filter_map(|item| {
            let PublicItem::InterfaceTable(table) = item else {
                return None;
            };
            Some(ConcreteFunctionIdentity {
                declaration: table.declaration,
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
                declaration: *implementation,
                arguments: arguments.clone(),
            }),
            _ => None,
        });
    let demands = owners
        .iter()
        .flat_map(|owner| owner.interface_instances.iter().cloned());
    for mut instance in allocations.chain(demands) {
        if ir
            .definitions()
            .resolve(instance.declaration)
            .expect("verified interface owner")
            .module()
            != &ir.identity
        {
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
    declaration: &DefinitionId,
    arguments: &[Ty<DefinitionId>],
) -> Vec<Ty<DefinitionId>> {
    if arguments.iter().all(Ty::is_concrete) {
        return arguments.to_vec();
    }
    ir.abi
        .public_items
        .iter()
        .find_map(|item| match item {
            PublicItem::InterfaceTable(table) if table.declaration == *declaration => {
                Some(table.generic_params.iter().map(|p| p.as_type()).collect())
            }
            _ => None,
        })
        .expect("verified interface template")
}

pub(super) fn collect_interface_tables(
    ir: &VerifiedMirModule,
    program: Option<&VerifiedMirProgram>,
    imports: &mut Vec<NativeImport<DefinitionId>>,
) -> Result<Vec<InterfaceTableRecord<DefinitionId>>, BytecodeLoweringError> {
    let closure = program
        .map(VerifiedMirProgram::modules)
        .unwrap_or(slice::from_ref(ir));
    let cancel = CancellationToken::default();
    let has_defaults = closure
        .iter()
        .flat_map(|module| &module.abi.public_items)
        .filter_map(|item| match item {
            PublicItem::InterfaceTable(table) => Some(table),
            _ => None,
        })
        .flat_map(|table| &table.methods)
        .any(|method| {
            matches!(
                method.implementation,
                CallableImplementation::NativeDefault(_)
            )
        });
    let default_contracts = if has_defaults {
        Some(Contracts::from_modules(closure, &cancel)?)
    } else {
        None
    };
    let catalog = default_contracts
        .as_ref()
        .map(|contracts| contracts.catalog(&cancel))
        .transpose()?;
    interface_instances(ir, program)
        .into_iter()
        .map(|instance| {
            let abi = ir
                .abi
                .public_items
                .iter()
                .find_map(|item| match item {
                    PublicItem::InterfaceTable(table)
                        if table.declaration == instance.declaration =>
                    {
                        Some(table.as_ref())
                    }
                    _ => None,
                })
                .expect("verified interface template");
            let Ty::Trait(interface) = &abi.trait_type else {
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
                let declaration = child(ir, abi.declaration, &segment)?;
                let member = child(ir, interface.declaration, &segment)?;
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
                            .instantiate_scoped(
                                &instance.arguments,
                                &abi.generic_params,
                                Some(ir.definitions()),
                            )
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
                            .instantiate_scoped(
                                &instance.arguments,
                                &abi.generic_params,
                                Some(ir.definitions()),
                            )
                            .ok_or(BytecodeLoweringError::InvalidNativeInterface)?
                            .bounds;
                        assumptions.extend(applied.bounds.clone());
                        let scope = abi
                            .generic_params
                            .iter()
                            .chain(&applied.generic_params)
                            .cloned()
                            .collect::<Vec<_>>();
                        let application = ir
                            .paths(application, &cancel)
                            .map_err(|_| BytecodeLoweringError::InvalidNativeInterface)?;
                        let scope = ir
                            .paths(&scope, &cancel)
                            .map_err(|_| BytecodeLoweringError::InvalidNativeInterface)?;
                        let assumptions = ir
                            .paths(&assumptions, &cancel)
                            .map_err(|_| BytecodeLoweringError::InvalidNativeInterface)?;
                        let instance = catalog
                            .as_ref()
                            .expect("default catalog")
                            .resolve_native_default_in(&application, &scope, &assumptions, &cancel)
                            .map_err(|_| BytecodeLoweringError::InvalidNativeInterface)?
                            .ok_or(BytecodeLoweringError::InvalidNativeInterface)?
                            .instance;
                        let mut instance = ir
                            .scope(&instance, &cancel)
                            .map_err(|_| BytecodeLoweringError::InvalidNativeInterface)?;
                        if !instance.arguments.is_empty() {
                            entry_arguments = instance.arguments.clone();
                            instance.arguments = (0..instance.arguments.len())
                                .map(|position| {
                                    GenericParam {
                                        owner: instance.declaration,
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
                                        GenericParam {
                                            owner: declaration,
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
                                method: member,
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

fn child(
    ir: &VerifiedMirModule,
    owner: DefinitionId,
    segment: &DefinitionPathSegment,
) -> Result<DefinitionId, BytecodeLoweringError> {
    ir.definitions()
        .lookup_child(owner, segment.kind, &segment.name, segment.occurrence)
        .ok_or(BytecodeLoweringError::InvalidNativeInterface)
}

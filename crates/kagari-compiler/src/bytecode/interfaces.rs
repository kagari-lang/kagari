//! Executable interface slots use checked declaration applications, without bodies
//! or per-library method selection for registered native entries.
use crate::bytecode::{BytecodeLoweringError, defaults};
use kagari_abi::{
    callable::CallableImplementation,
    ids::FunctionRef,
    native_import::NativeImport,
    types::{AbiType, ConcreteFunctionIdentity, PublicAbiItem},
};
use kagari_bytecode::{
    instruction::NativeImportId,
    module::{CallableTarget, InterfaceMethodSlot, InterfaceTableRecord},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionId, DefinitionKind, DefinitionPathSegment},
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
    for instance in allocations.chain(demands) {
        if instance.declaration.module == ir.identity && !instances.contains(&instance) {
            instances.push(instance);
        }
    }
    instances
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
            for method in &abi.methods {
                let segment = DefinitionPathSegment {
                    kind: DefinitionKind::Method,
                    name: method.name.clone(),
                    occurrence: 0,
                };
                let declaration = child(&abi.declaration, segment.clone());
                let member = child(&interface.declaration, segment);
                if matches!(
                    method.implementation,
                    CallableImplementation::Native(_) | CallableImplementation::NativeDefault(_)
                ) {
                    if !method.generic_params.is_empty()
                        || (instance.arguments.is_empty() && !abi.generic_params.is_empty())
                    {
                        continue;
                    }
                    let target = if matches!(
                        method.implementation,
                        CallableImplementation::NativeDefault(_)
                    ) {
                        let applied = abi
                            .instantiate(&instance.arguments)
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
                        catalog
                            .as_ref()
                            .expect("default catalog")
                            .resolve_native_default(application, &cancel)
                            .map_err(|_| BytecodeLoweringError::InvalidNativeInterface)?
                            .ok_or(BytecodeLoweringError::InvalidNativeInterface)?
                            .instance
                    } else {
                        ConcreteFunctionIdentity {
                            declaration,
                            arguments: instance.arguments.clone(),
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
                    });
                } else {
                    for function in &ir.functions {
                        if function.instance.declaration == declaration
                            && function.instance.arguments == instance.arguments
                        {
                            methods.push(InterfaceMethodSlot {
                                method: member.clone(),
                                target: CallableTarget::Script(FunctionRef::new(
                                    function.id.index(),
                                )),
                            });
                        }
                    }
                }
            }
            Ok(InterfaceTableRecord {
                declaration: instance.declaration,
                arguments: instance.arguments,
                methods,
            })
        })
        .collect()
}

fn child(owner: &DefinitionId, segment: DefinitionPathSegment) -> DefinitionId {
    let mut declaration = owner.clone();
    declaration.path.push(segment);
    declaration
}

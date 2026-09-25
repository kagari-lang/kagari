use super::{Context, IrVerificationError, IrVerificationErrorKind as Error};
use crate::module::{
    CallTarget, Constant, Instruction, IrFunction, IrModule, ValueType,
    contracts::{self, ContractError},
    instruction::RuntimeHelper,
};

pub(super) fn verify(
    module: &IrModule,
    function: &IrFunction,
    instruction: &Instruction,
    context: Context<'_>,
) -> Result<(), IrVerificationError> {
    use Instruction::*;
    let contract = |error| context.error(Error::Contract(error));
    if let Some(path) = instruction.path_reference()
        && let Some(declaration) = &path.declaration
    {
        let catalog = kagari_common::host_interface::HostInterface {
            types: module.host_types.clone(),
            ..Default::default()
        };
        let resolved = declaration
            .contract(&catalog)
            .map_err(|_| context.error(Error::InvalidHostInterface))?;
        if resolved
            .fingerprint()
            .map_err(|_| context.error(Error::InvalidHostInterface))?
            != path.contract_fingerprint
            || path.root_ty != ValueType::HostHandle
            || path.result_ty != ValueType::from_host_type(&resolved.result)
            || (!path.read_only
                && declaration.access != kagari_common::host_interface::PathAccess::ReadWrite)
        {
            return Err(context.error(Error::InvalidHostInterface));
        }
    }
    match instruction {
        LoadConst { dst, constant } => {
            let ty = match constant {
                Constant::Unit => ValueType::Unit,
                Constant::Bool(_) => ValueType::Bool,
                Constant::I32(_) => ValueType::I32,
                Constant::F32(_) => ValueType::F32,
                Constant::Str(_) => ValueType::Str,
            };
            context.expect(dst.ty, ty, "constant destination")?;
        }
        LoadLocal { dst, local } => {
            context.expect(dst.ty, context.local(function, *local)?, "local load")?
        }
        StoreLocal { local, src } => {
            context.expect(src.ty, context.local(function, *local)?, "local store")?
        }
        LoadModule { dst, slot } => {
            let slot = module
                .module_slots
                .get(slot.index())
                .ok_or_else(|| context.error(Error::InvalidModuleSlot))?;
            context.expect(dst.ty, slot.ty, "module load")?;
        }
        StoreModule { slot, src } => {
            let slot = module
                .module_slots
                .get(slot.index())
                .ok_or_else(|| context.error(Error::InvalidModuleSlot))?;
            context.expect(src.ty, slot.ty, "module store")?;
        }
        Move { dst, src } => context.expect(dst.ty, src.ty, "move destination")?,
        Unary { dst, op, operand } => context.expect(
            dst.ty,
            contracts::unary_result(*op, operand.ty).map_err(contract)?,
            "unary destination",
        )?,
        Binary { dst, op, lhs, rhs } => context.expect(
            dst.ty,
            contracts::binary_result(*op, lhs.ty, rhs.ty).map_err(contract)?,
            "binary destination",
        )?,
        Call { dst, callee, args } => match callee {
            CallTarget::Function(target) => {
                let callee = module
                    .functions
                    .get(target.index())
                    .ok_or_else(|| context.error(Error::InvalidCall(*target)))?;
                if args.len() != callee.params.len() {
                    return Err(context.error(Error::CallArity {
                        expected: callee.params.len(),
                        found: args.len(),
                    }));
                }
                for (arg, param) in args.iter().zip(&callee.params) {
                    context.expect(arg.ty, param.ty, "call argument")?;
                }
                contracts::verify_call_dst(dst.map(|v| v.ty), callee.return_type)
                    .map_err(contract)?;
            }
            CallTarget::SourceFunction(callee) => {
                if args.len() != callee.params.len() {
                    return Err(context.error(Error::CallArity {
                        expected: callee.params.len(),
                        found: args.len(),
                    }));
                }
                for (arg, param) in args.iter().zip(&callee.params) {
                    context.expect(arg.ty, *param, "source call argument")?;
                }
                contracts::verify_call_dst(dst.map(|v| v.ty), callee.return_type)
                    .map_err(contract)?;
            }
            CallTarget::InterfaceMethod(interface_call) => {
                if interface_call.interface.declaration.module == module.identity {
                    let (params, return_type) = super::super::abi::interface_method_types(
                        &module.identity,
                        &module.abi.public_items,
                        &module.abi.trait_contracts,
                        &interface_call.interface,
                        interface_call.method_slot as usize,
                    )
                    .ok_or_else(|| context.error(Error::InvalidInterfaceTable))?;
                    if args.len() != params.len() {
                        return Err(context.error(Error::CallArity {
                            expected: params.len(),
                            found: args.len(),
                        }));
                    }
                    for (arg, param) in args.iter().zip(params) {
                        context.expect(arg.ty, param, "interface call argument")?;
                    }
                    contracts::verify_call_dst(dst.map(|v| v.ty), return_type).map_err(contract)?;
                } else {
                    // Whole-program verification checks the imported declaration
                    // and complete method signature before bytecode lowering.
                    let receiver = args
                        .first()
                        .ok_or_else(|| context.error(Error::InvalidInterfaceTable))?;
                    context.expect(receiver.ty, ValueType::HeapObject, "interface receiver")?;
                }
            }
            CallTarget::StandardIntrinsic(intrinsic) => contracts::verify_intrinsic(
                dst.map(|v| v.ty),
                *intrinsic,
                &args.iter().map(|v| v.ty).collect::<Vec<_>>(),
            )
            .map_err(contract)?,
            CallTarget::HostFunction(declaration) => contracts::verify_host_call(
                dst.map(|v| v.ty),
                declaration,
                &args.iter().map(|v| v.ty).collect::<Vec<_>>(),
            )
            .map_err(contract)?,
            CallTarget::Value(_) | CallTarget::RuntimeHelper(RuntimeHelper::DynamicCall) => {
                return Err(context.error(Error::UnsupportedCall));
            }
            CallTarget::RuntimeHelper(helper) => {
                let kind = match helper {
                    RuntimeHelper::ReflectTypeOf => contracts::RuntimeHelperKind::TypeOf,
                    RuntimeHelper::ReflectGetField(_) => contracts::RuntimeHelperKind::GetField,
                    RuntimeHelper::ReflectSetField(_) => contracts::RuntimeHelperKind::SetField,
                    RuntimeHelper::ReflectSetIndex => contracts::RuntimeHelperKind::SetIndex,
                    RuntimeHelper::DynamicCall => unreachable!(),
                };
                contracts::verify_runtime_helper_call(
                    dst.map(|value| value.ty),
                    kind,
                    &args.iter().map(|value| value.ty).collect::<Vec<_>>(),
                )
                .map_err(contract)?;
            }
        },
        MakeTuple { dst, .. } | MakeArray { dst, .. } => {
            context.expect(dst.ty, ValueType::HeapObject, "aggregate destination")?
        }
        MakeInterface {
            dst,
            value,
            implementation,
        } => {
            use crate::module::PublicAbiItem;
            context.expect(dst.ty, ValueType::HeapObject, "interface destination")?;
            let table = module.abi.public_items.iter().find_map(|item| match item {
                PublicAbiItem::InterfaceTable(table) if &table.declaration == implementation => {
                    Some(table)
                }
                _ => None,
            });
            let Some(table) = table else {
                if implementation.module != module.identity {
                    // The complete program verifier proves imported table identity,
                    // signature and dependency reachability.
                    return Ok(());
                }
                return Err(context.error(Error::InvalidInterfaceTable));
            };
            if !table.generic_params.is_empty()
                || !table.for_type.is_concrete()
                || !table.trait_type.is_concrete()
                || table
                    .methods
                    .iter()
                    .any(|method| !method.generic_params.is_empty())
            {
                return Err(context.error(Error::InvalidInterfaceTable));
            }
            context.expect(
                value.ty,
                table.for_type.representation(),
                "interface receiver",
            )?;
        }
        MakeEnum {
            dst,
            enumeration,
            variant,
            fields,
        } => {
            context.expect(dst.ty, ValueType::HeapObject, "enum destination")?;
            let variant = module
                .enumerations
                .iter()
                .find(|layout| {
                    layout.declaration == enumeration.declaration
                        && layout.arguments == enumeration.arguments
                })
                .and_then(|layout| layout.variants.get(*variant))
                .ok_or_else(|| context.error(Error::InvalidEnumInitializer))?;
            if fields.len() != variant.payload.len() {
                return Err(context.error(Error::InvalidEnumInitializer));
            }
            for (value, ty) in fields.iter().zip(&variant.payload) {
                context.check_cancel()?;
                context.expect(value.ty, ty.representation(), "enum payload")?;
            }
        }
        MakeStruct {
            dst,
            structure,
            fields,
        } => {
            context.expect(dst.ty, ValueType::HeapObject, "struct destination")?;
            let layout = module
                .structure(structure)
                .ok_or_else(|| context.error(Error::InvalidStructInitializer))?;
            if fields.len() != layout.fields.len() {
                return Err(context.error(Error::InvalidStructInitializer));
            }
            let mut seen = std::collections::HashSet::new();
            for field in fields {
                context.check_cancel()?;
                let target = layout
                    .fields
                    .get(field.slot)
                    .ok_or_else(|| context.error(Error::InvalidStructInitializer))?;
                if !seen.insert(field.slot) {
                    return Err(context.error(Error::InvalidStructInitializer));
                }
                context.expect(
                    field.value.ty,
                    target.ty.representation(),
                    "struct field initializer",
                )?;
            }
        }
        ReadAggregateField { dst, base, field } => {
            context.expect(base.ty, ValueType::HeapObject, "field base")?;
            let target = module
                .structure(&field.owner)
                .and_then(|layout| layout.fields.get(field.slot))
                .ok_or_else(|| context.error(Error::InvalidField))?;
            context.expect(dst.ty, target.ty.representation(), "field read")?;
        }
        WriteAggregateField { base, field, value } => {
            context.expect(base.ty, ValueType::HeapObject, "field base")?;
            let target = module
                .structure(&field.owner)
                .and_then(|layout| layout.fields.get(field.slot))
                .ok_or_else(|| context.error(Error::InvalidField))?;
            if !target.mutable {
                return Err(context.error(Error::ReadOnlyField));
            }
            context.expect(value.ty, target.ty.representation(), "field write")?;
        }
        ReadAggregateIndex { base, index, .. } | WriteAggregateIndex { base, index, .. } => {
            context.expect(base.ty, ValueType::HeapObject, "index base")?;
            if !matches!(index.ty, ValueType::I32 | ValueType::I64) {
                return Err(contract(ContractError::InvalidOperation {
                    reason: "aggregate index must have integer representation",
                }));
            }
        }
        ReadPath {
            dst,
            root_or_view,
            path,
            ..
        } => {
            context.expect(path.root_ty, ValueType::HostHandle, "path representation")?;
            context.expect(root_or_view.ty, path.root_ty, "path root")?;
            context.expect(dst.ty, path.result_ty, "path result")?;
        }
        MakePathView {
            dst,
            root_or_view,
            path,
            ..
        } => {
            context.expect(path.root_ty, ValueType::HostHandle, "path representation")?;
            context.expect(root_or_view.ty, path.root_ty, "path root")?;
            context.expect(dst.ty, ValueType::HostHandle, "path view")?;
        }
        SetPath {
            root_or_view,
            path,
            value,
            ..
        }
        | ModifyPath {
            root_or_view,
            path,
            value,
            ..
        } => {
            if path.read_only {
                return Err(context.error(Error::ReadOnlyPath));
            }
            context.expect(path.root_ty, ValueType::HostHandle, "path representation")?;
            context.expect(root_or_view.ty, path.root_ty, "path root")?;
            context.expect(value.ty, path.result_ty, "path value")?;
            if let ModifyPath { dst, op, .. } = instruction {
                let ty =
                    contracts::binary_result(*op, path.result_ty, value.ty).map_err(contract)?;
                context.expect(ty, path.result_ty, "path modification result")?;
                if let Some(dst) = dst {
                    context.expect(dst.ty, ty, "path modification destination")?;
                }
            }
        }
    }
    Ok(())
}

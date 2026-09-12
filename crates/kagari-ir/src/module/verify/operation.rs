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
        Call { dst, callee, args } => {
            match callee {
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
                CallTarget::StandardIntrinsic(intrinsic) => contracts::verify_intrinsic(
                    dst.map(|v| v.ty),
                    *intrinsic,
                    &args.iter().map(|v| v.ty).collect::<Vec<_>>(),
                )
                .map_err(contract)?,
                CallTarget::Value(_) | CallTarget::RuntimeHelper(RuntimeHelper::DynamicCall) => {
                    return Err(context.error(Error::UnsupportedCall));
                }
                CallTarget::RuntimeHelper(helper) => {
                    let arity = match helper {
                        RuntimeHelper::HostFunction(_) => None, // R06 links the offline declaration signature.
                        RuntimeHelper::ReflectTypeOf | RuntimeHelper::ReflectGetField(_) => Some(1),
                        RuntimeHelper::ReflectSetField(_) => Some(2),
                        RuntimeHelper::ReflectSetIndex => Some(3),
                        RuntimeHelper::DynamicCall => unreachable!(),
                    };
                    if let Some(expected) = arity {
                        if args.len() != expected {
                            return Err(context.error(Error::CallArity {
                                expected,
                                found: args.len(),
                            }));
                        }
                        if matches!(helper, RuntimeHelper::ReflectTypeOf) {
                            contracts::verify_call_dst(dst.map(|v| v.ty), ValueType::Str)
                                .map_err(contract)?;
                        }
                    }
                }
            }
        }
        MakeTuple { dst, .. } | MakeArray { dst, .. } | MakeStruct { dst, .. } => {
            context.expect(dst.ty, ValueType::HeapObject, "aggregate destination")?
        }
        ReadAggregateField { base, .. } | WriteAggregateField { base, .. } => {
            context.expect(base.ty, ValueType::HeapObject, "field base")?
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
            context.expect(root_or_view.ty, path.root_ty, "path root")?;
            context.expect(dst.ty, path.result_ty, "path result")?;
        }
        MakePathView {
            dst,
            root_or_view,
            path,
            ..
        } => {
            context.expect(root_or_view.ty, path.root_ty, "path root")?;
            context.expect(dst.ty, ValueType::HeapObject, "path view")?;
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

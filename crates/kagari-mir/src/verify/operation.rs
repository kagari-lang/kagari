use crate::{
    function::{MirFunction, MirModule},
    instruction::{CallTarget, Constant, Instruction, RuntimeHelper},
    verify::{
        Context, MirVerificationError, MirVerificationErrorKind as Error, suspension::ephemeral,
    },
};
use kagari_abi::representation::ValueType;
use kagari_contract::{
    contracts,
    contracts::{ContractError, RuntimeHelperKind},
    operations,
    operations::range_operands_valid,
    representation::{host_representation, semantic_representation},
    types::{PublicItem, type_contract},
};
use kagari_types::{
    declaration::{TypeDefKind, native::NativeStorageLayout, verify::types_in_scope},
    host_interface::{HostInterface, type_declaration::PathAccess},
    ty::Ty,
};
use std::collections::HashSet;

pub(super) fn verify(
    module: &MirModule,
    function: &MirFunction,
    instruction: &Instruction,
    context: Context<'_>,
) -> Result<(), MirVerificationError> {
    let contract = |error| context.error(Error::Contract(error));
    if let Some(path) = instruction.path_reference()
        && let Some(declaration) = &path.declaration
    {
        let catalog = HostInterface {
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
            || path.result_ty != host_representation(&resolved.result)
            || (!path.read_only && declaration.access != PathAccess::ReadWrite)
        {
            return Err(context.error(Error::InvalidHostInterface));
        }
    }
    let aggregate = match instruction {
        Instruction::MakeStruct { structure, .. } => Some(structure),
        Instruction::MakeEnum { enumeration, .. }
        | Instruction::TestEnumVariant { enumeration, .. }
        | Instruction::ReadEnumPayload { enumeration, .. } => Some(enumeration),
        Instruction::ReadAggregateField { field, .. }
        | Instruction::WriteAggregateField { field, .. } => Some(&field.owner),
        _ => None,
    };
    if let Some(aggregate) = aggregate
        && (!aggregate.associated_types.is_empty()
            || !types_in_scope(
                &aggregate.arguments,
                function
                    .semantic
                    .generic
                    .as_ref()
                    .map_or(&[], |body| body.parameters.as_slice()),
                context.cancel,
            ))
    {
        return Err(context.error(Error::InvalidStructInitializer));
    }
    match instruction {
        Instruction::MakeFuture {
            dst,
            function: target,
            arguments,
            future,
        } => {
            let invalid = || context.error(Error::InvalidCall(*target));
            let callee = module.functions.get(target.index()).ok_or_else(invalid)?;
            let Ty::NativeObject(nominal) = future else {
                return Err(invalid());
            };
            if nominal.arguments.len() != 1
                || !nominal.associated_types.is_empty()
                || function.semantic.registers.get(&dst.temp.index()) != Some(future)
                || !callee.effects.may_suspend
                || callee.semantic.result.as_ref() != nominal.arguments.first()
                || arguments.len() != callee.params.len()
                || (callee.semantic.generic.is_some()
                    && callee.semantic.generic != function.semantic.generic)
            {
                return Err(invalid());
            }
            if nominal.declaration.module == module.identity
                && type_contract(
                    &module.identity,
                    &module.abi.public_items,
                    &nominal.declaration,
                )
                .is_none_or(|ty| ty.kind != TypeDefKind::NativeStorage(NativeStorageLayout::Future))
            {
                return Err(invalid());
            }
            context.expect(dst.ty, ValueType::HeapObject, "Future destination")?;
            for (index, (argument, parameter)) in arguments.iter().zip(&callee.params).enumerate() {
                context.expect(argument.ty, parameter.ty, "Future capture")?;
                if ephemeral(argument.ty, callee.semantic.params.get(&index))
                    || !callee.semantic.params.contains_key(&index)
                    || function.semantic.registers.get(&argument.temp.index())
                        != callee.semantic.params.get(&index)
                {
                    return Err(invalid());
                }
            }
        }
        Instruction::Await { dst, value, future } => {
            let invalid = || {
                contract(ContractError::InvalidOperation {
                    reason: "invalid Future or Task await contract",
                })
            };
            let Ty::NativeObject(nominal) = future else {
                return Err(invalid());
            };
            if nominal.arguments.len() != 1
                || !nominal.associated_types.is_empty()
                || function.semantic.registers.get(&value.temp.index()) != Some(future)
                || function.semantic.registers.get(&dst.temp.index()) != nominal.arguments.first()
            {
                return Err(invalid());
            }
            if nominal.declaration.module == module.identity {
                let ty = type_contract(
                    &module.identity,
                    &module.abi.public_items,
                    &nominal.declaration,
                );
                if ty.is_none_or(|ty| {
                    !matches!(
                        ty.kind,
                        TypeDefKind::NativeStorage(
                            NativeStorageLayout::Future | NativeStorageLayout::Task
                        )
                    )
                }) {
                    return Err(invalid());
                }
            }
            // Imported roles are checked in the complete program dependency closure.
            context.expect(value.ty, ValueType::HeapObject, "awaitable")?;
            context.expect(
                dst.ty,
                semantic_representation(&nominal.arguments[0]),
                "await output",
            )?;
        }
        Instruction::Convert {
            dst,
            src,
            conversion,
        } => {
            let (input, output) = conversion.contract().ok_or_else(|| {
                contract(ContractError::InvalidOperation {
                    reason: "invalid numeric conversion",
                })
            })?;
            context.expect(src.ty, semantic_representation(&input), "conversion source")?;
            context.expect(
                dst.ty,
                semantic_representation(&output),
                "conversion destination",
            )?;
        }
        Instruction::Numeric {
            dst,
            operation,
            lhs,
            rhs,
        } => {
            let (left, right, result) = operation.contract().ok_or_else(|| {
                contract(ContractError::InvalidOperation {
                    reason: "invalid numeric contract",
                })
            })?;
            context.expect(lhs.ty, semantic_representation(&left), "numeric input")?;
            context.expect(dst.ty, semantic_representation(&result), "numeric output")?;
            match (right, rhs) {
                (Some(ty), Some(value)) => {
                    context.expect(value.ty, semantic_representation(&ty), "numeric rhs")?
                }
                (None, None) => {}
                _ => {
                    return Err(contract(ContractError::InvalidOperation {
                        reason: "numeric arity",
                    }));
                }
            }
        }
        Instruction::LoadConst { dst, constant } => {
            let ty = match constant {
                Constant::Unit => ValueType::Unit,
                Constant::Bool(_) => ValueType::Bool,
                Constant::I32(_) => ValueType::I32,
                Constant::I64(_) => ValueType::I64,
                Constant::F32(_) => ValueType::F32,
                Constant::F64(_) => ValueType::F64,
                Constant::U64(_) => ValueType::U64,
                Constant::Str(_) => ValueType::Str,
            };
            context.expect(dst.ty, ty, "constant destination")?;
        }
        Instruction::BeginIteration { collection } => {
            context.expect(collection.ty, ValueType::HeapObject, "iteration collection")?;
        }
        Instruction::EndIteration => {}
        Instruction::LoadLocal { dst, local } => {
            context.expect(dst.ty, context.local(function, *local)?, "local load")?
        }
        Instruction::StoreLocal { local, src } => {
            context.expect(src.ty, context.local(function, *local)?, "local store")?
        }
        Instruction::LoadModule { dst, slot } => {
            let slot = module
                .module_slots
                .get(slot.index())
                .ok_or_else(|| context.error(Error::InvalidModuleSlot))?;
            context.expect(dst.ty, slot.ty, "module load")?;
        }
        Instruction::StoreModule { slot, src } => {
            let slot = module
                .module_slots
                .get(slot.index())
                .ok_or_else(|| context.error(Error::InvalidModuleSlot))?;
            context.expect(src.ty, slot.ty, "module store")?;
        }
        Instruction::Move { dst, src } => context.expect(dst.ty, src.ty, "move destination")?,
        Instruction::Unary { dst, op, operand } => context.expect(
            dst.ty,
            contracts::unary_result(*op, operand.ty).map_err(contract)?,
            "unary destination",
        )?,
        Instruction::Binary { dst, op, lhs, rhs } => context.expect(
            dst.ty,
            contracts::binary_result(*op, lhs.ty, rhs.ty).map_err(contract)?,
            "binary destination",
        )?,
        Instruction::Call { dst, callee, args } => match callee {
            CallTarget::Shared(call) => {
                if !call.structurally_valid(
                    function
                        .semantic
                        .generic
                        .as_ref()
                        .map_or(&[], |body| body.parameters.as_slice()),
                    &Default::default(),
                ) {
                    return Err(context.error(Error::UnsupportedCall));
                }
                if args.len() != call.signature.params.len() {
                    return Err(context.error(Error::CallArity {
                        expected: call.signature.params.len(),
                        found: args.len(),
                    }));
                }
                for (arg, param) in args.iter().zip(&call.signature.params) {
                    context.expect(
                        arg.ty,
                        semantic_representation(param),
                        "shared call argument",
                    )?;
                }
                contracts::verify_call_dst(
                    dst.map(|v| v.ty),
                    semantic_representation(&call.signature.result),
                )
                .map_err(contract)?;
            }
            CallTarget::Function(target) => {
                let callee = module
                    .functions
                    .get(target.index())
                    .ok_or_else(|| context.error(Error::InvalidCall(*target)))?;
                if callee.semantic.generic.is_some() || callee.effects.may_suspend {
                    return Err(context.error(Error::InvalidCall(*target)));
                }
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
                    let (params, return_type) = interface_call
                        .signature_in(
                            &module.identity,
                            &module.abi.public_items,
                            &module.abi.trait_contracts,
                            &Default::default(),
                        )
                        .ok()
                        .and_then(|signature| {
                            signature.physical_types(
                                function
                                    .semantic
                                    .generic
                                    .as_ref()
                                    .map_or(&[], |body| body.parameters.as_slice()),
                                &Default::default(),
                            )
                        })
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
                    context.expect(
                        receiver.ty,
                        interface_call
                            .receiver
                            .as_ref()
                            .map_or(ValueType::HeapObject, semantic_representation),
                        "interface receiver",
                    )?;
                }
            }
            CallTarget::RuntimePrimitive(intrinsic) => contracts::verify_intrinsic(
                dst.map(|v| v.ty),
                *intrinsic,
                &args.iter().map(|v| v.ty).collect::<Vec<_>>(),
            )
            .map_err(contract)?,
            CallTarget::Native(import) => contracts::verify_native_call(
                dst.map(|v| v.ty),
                import,
                &args.iter().map(|v| v.ty).collect::<Vec<_>>(),
            )
            .map_err(contract)?,

            CallTarget::Value(_) | CallTarget::RuntimeHelper(RuntimeHelper::DynamicCall) => {
                return Err(context.error(Error::UnsupportedCall));
            }
            CallTarget::Closure {
                value,
                params,
                return_type,
            } => {
                context.expect(value.ty, ValueType::HeapObject, "closure callee")?;
                if args.len() != params.len() {
                    return Err(context.error(Error::CallArity {
                        expected: params.len(),
                        found: args.len(),
                    }));
                }
                for (arg, ty) in args.iter().zip(params) {
                    context.expect(arg.ty, *ty, "closure argument")?;
                }
                contracts::verify_call_dst(dst.map(|v| v.ty), *return_type).map_err(contract)?;
            }
            CallTarget::RuntimeHelper(helper) => {
                let kind = match helper {
                    RuntimeHelper::ReflectTypeOf => RuntimeHelperKind::TypeOf,
                    RuntimeHelper::ReflectGetField(_) => RuntimeHelperKind::GetField,
                    RuntimeHelper::ReflectSetField(_) => RuntimeHelperKind::SetField,
                    RuntimeHelper::ReflectSetIndex => RuntimeHelperKind::SetIndex,
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
        Instruction::RangeBound {
            dst,
            value,
            range,
            bound,
            ..
        } => {
            context.expect(dst.ty, ValueType::HeapObject, "bound destination")?;
            context.expect(value.ty, ValueType::HeapObject, "bound range")?;
            if !operations::range_bound_valid(range, bound) {
                return Err(contract(ContractError::InvalidOperation {
                    reason: "invalid range bound contract",
                }));
            }
        }
        Instruction::MakeRange {
            dst,
            start,
            end,
            ty,
        } => {
            context.expect(dst.ty, ValueType::HeapObject, "range destination")?;
            if !range_operands_valid(ty, start.map(|v| v.ty), end.map(|v| v.ty)) {
                return Err(contract(ContractError::InvalidOperation {
                    reason: "invalid range operands",
                }));
            }
        }
        Instruction::RepeatArray {
            dst,
            element,
            value,
            count,
        } => {
            context.expect(dst.ty, ValueType::HeapObject, "repeat array destination")?;
            context.expect(count.ty, ValueType::U64, "repeat array count")?;
            context.expect(
                value.ty,
                semantic_representation(element),
                "repeat array element",
            )?;
            if !element.within_wire_limits()
                || (!element.is_concrete()
                    && !function
                        .semantic
                        .generic
                        .as_ref()
                        .is_some_and(|body| body.types_valid([element], &Default::default())))
            {
                return Err(contract(ContractError::InvalidOperation {
                    reason: "unresolved or oversized array element",
                }));
            }
        }
        Instruction::MakeArray {
            dst,
            element,
            elements,
        } => {
            context.expect(dst.ty, ValueType::HeapObject, "array destination")?;
            if !element.within_wire_limits()
                || (!element.is_concrete()
                    && !function
                        .semantic
                        .generic
                        .as_ref()
                        .is_some_and(|body| body.types_valid([element], &Default::default())))
            {
                return Err(contract(ContractError::InvalidOperation {
                    reason: "unresolved or oversized array element",
                }));
            }
            for value in elements {
                context.expect(value.ty, semantic_representation(element), "array element")?;
            }
        }
        Instruction::MakeTuple { dst, .. } => {
            context.expect(dst.ty, ValueType::HeapObject, "aggregate destination")?
        }
        Instruction::MakeClosure {
            dst,
            function: target,
            captures,
        } => {
            context.expect(dst.ty, ValueType::HeapObject, "closure destination")?;
            let callee = module
                .functions
                .get(target.index())
                .ok_or_else(|| context.error(Error::InvalidCall(*target)))?;
            if callee.effects.may_suspend {
                return Err(context.error(Error::InvalidCall(*target)));
            }
            if captures.len() > callee.params.len() {
                return Err(context.error(Error::CallArity {
                    expected: callee.params.len(),
                    found: captures.len(),
                }));
            }
            if callee.semantic.generic.is_some()
                && callee.semantic.generic != function.semantic.generic
            {
                return Err(context.error(Error::InvalidInterfaceTable));
            }
            for (capture, param) in captures.iter().zip(&callee.params) {
                context.expect(capture.ty, param.ty, "closure capture")?;
            }
        }
        Instruction::MakeCell { dst, .. } => {
            context.expect(dst.ty, ValueType::HeapObject, "cell destination")?
        }
        Instruction::ReadCell { cell, .. } | Instruction::WriteCell { cell, .. } => {
            context.expect(cell.ty, ValueType::HeapObject, "cell handle")?
        }
        Instruction::UpcastInterface {
            dst,
            value,
            source,
            target,
        } => {
            context.expect(dst.ty, ValueType::HeapObject, "interface destination")?;
            context.expect(value.ty, ValueType::HeapObject, "interface receiver")?;
            if !types_in_scope(
                [&Ty::Trait(source.clone()), &Ty::Trait(target.clone())],
                function
                    .semantic
                    .generic
                    .as_ref()
                    .map_or(&[], |body| body.parameters.as_slice()),
                context.cancel,
            ) {
                return Err(context.error(Error::InvalidInterfaceTable));
            }
        }
        Instruction::MakeInterface {
            dst,
            value,
            implementation,
            arguments,
        } => {
            context.expect(dst.ty, ValueType::HeapObject, "interface destination")?;
            let table = module.abi.public_items.iter().find_map(|item| match item {
                PublicItem::InterfaceTable(table) if &table.declaration == implementation => {
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
            let table = table
                .instantiate_in(
                    arguments,
                    function
                        .semantic
                        .generic
                        .as_ref()
                        .map_or(&[], |body| &body.parameters),
                )
                .ok_or_else(|| context.error(Error::InvalidInterfaceTable))?;
            context.expect(
                value.ty,
                semantic_representation(&table.for_type),
                "interface receiver",
            )?;
        }
        Instruction::Iter { dst, value, ty, op } => {
            let (input, output) = op
                .contract(ty)
                .ok_or_else(|| context.error(Error::InvalidEnumInitializer))?;
            if input != value.map(|v| v.ty) {
                return Err(context.error(Error::InvalidEnumInitializer));
            }
            context.expect(dst.ty, output, "iterator result")?;
        }

        Instruction::MakeEnum {
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
                        && layout.accepts(&enumeration.arguments)
                })
                .and_then(|layout| layout.apply(&enumeration.arguments, context.cancel))
                .and_then(|layout| layout.variants.get(*variant).cloned())
                .ok_or_else(|| context.error(Error::InvalidEnumInitializer))?;
            if fields.len() != variant.payload.len() {
                return Err(context.error(Error::InvalidEnumInitializer));
            }
            for (value, ty) in fields.iter().zip(&variant.payload) {
                context.check_cancel()?;
                context.expect(value.ty, semantic_representation(ty), "enum payload")?;
            }
        }
        Instruction::TestEnumVariant {
            dst,
            value,
            enumeration,
            variant,
        } => {
            context.expect(value.ty, ValueType::HeapObject, "enum pattern value")?;
            context.expect(dst.ty, ValueType::Bool, "enum pattern result")?;
            module
                .enumerations
                .iter()
                .find(|layout| {
                    layout.declaration == enumeration.declaration
                        && layout.accepts(&enumeration.arguments)
                })
                .and_then(|layout| layout.apply(&enumeration.arguments, context.cancel))
                .and_then(|layout| layout.variants.get(*variant).cloned())
                .ok_or_else(|| context.error(Error::InvalidEnumInitializer))?;
        }
        Instruction::ReadEnumPayload {
            dst,
            value,
            enumeration,
            variant,
            index,
        } => {
            context.expect(value.ty, ValueType::HeapObject, "enum pattern value")?;
            let payload = module
                .enumerations
                .iter()
                .find(|layout| {
                    layout.declaration == enumeration.declaration
                        && layout.accepts(&enumeration.arguments)
                })
                .and_then(|layout| layout.apply(&enumeration.arguments, context.cancel))
                .and_then(|layout| layout.variants.get(*variant).cloned())
                .and_then(|variant| variant.payload.get(*index).cloned())
                .ok_or_else(|| context.error(Error::InvalidEnumInitializer))?;
            context.expect(
                dst.ty,
                semantic_representation(&payload),
                "enum pattern payload",
            )?;
        }
        Instruction::MakeStruct {
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
            let mut seen = HashSet::new();
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
                    semantic_representation(&target.ty),
                    "struct field initializer",
                )?;
            }
        }
        Instruction::ReadAggregateField { dst, base, field } => {
            context.expect(base.ty, ValueType::HeapObject, "field base")?;
            let target = module
                .structure(&field.owner)
                .and_then(|layout| layout.fields.get(field.slot).cloned())
                .ok_or_else(|| context.error(Error::InvalidField))?;
            context.expect(dst.ty, semantic_representation(&target.ty), "field read")?;
        }
        Instruction::WriteAggregateField { base, field, value } => {
            context.expect(base.ty, ValueType::HeapObject, "field base")?;
            let target = module
                .structure(&field.owner)
                .and_then(|layout| layout.fields.get(field.slot).cloned())
                .ok_or_else(|| context.error(Error::InvalidField))?;
            if !target.mutable {
                return Err(context.error(Error::ReadOnlyField));
            }
            context.expect(value.ty, semantic_representation(&target.ty), "field write")?;
        }
        Instruction::ReadAggregateIndex { base, index, .. }
        | Instruction::WriteAggregateIndex { base, index, .. } => {
            context.expect(base.ty, ValueType::HeapObject, "index base")?;
            if !matches!(index.ty, ValueType::I32 | ValueType::I64 | ValueType::U64) {
                return Err(contract(ContractError::InvalidOperation {
                    reason: "aggregate index must have integer representation",
                }));
            }
        }
        Instruction::ReadPath {
            dst,
            root_or_view,
            path,
            ..
        } => {
            context.expect(path.root_ty, ValueType::HostHandle, "path representation")?;
            context.expect(root_or_view.ty, path.root_ty, "path root")?;
            context.expect(dst.ty, path.result_ty, "path result")?;
        }
        Instruction::MakePathView {
            dst,
            root_or_view,
            path,
            ..
        } => {
            context.expect(path.root_ty, ValueType::HostHandle, "path representation")?;
            context.expect(root_or_view.ty, path.root_ty, "path root")?;
            context.expect(dst.ty, ValueType::HostHandle, "path view")?;
        }
        Instruction::SetPath {
            root_or_view,
            path,
            value,
            ..
        }
        | Instruction::ModifyPath {
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
            if let Instruction::ModifyPath { dst, op, .. } = instruction {
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

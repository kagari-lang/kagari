use crate::{
    instruction::{BytecodeInstruction, CallTarget, Register, RuntimeHelper, UnaryOp},
    module::{BytecodeFunction, BytecodeModule},
    program::BytecodeProgram,
    trait_bounds::shared,
    verifier::{
        BytecodeVerificationError, constant_type, contract_error, expect_register_ty, field_layout,
        flow::inline_host, function_ref_exists, ir_binary_op, local_ty, module_slot_ty,
        path_record, register_ty, verify_call_dst, verify_dynamic_path_args, verify_jump,
        verify_standard_intrinsic_call,
    },
};
use kagari_abi::representation::ValueType;
use kagari_contract::{
    contracts,
    contracts::RuntimeHelperKind,
    operations,
    operations::UnaryOp as MirUnaryOp,
    representation::semantic_representation,
    types::{PublicItem, type_contract},
};
use kagari_types::{
    declaration::{TypeDefKind, native::NativeStorageLayout, verify::types_in_scope},
    ty::Ty,
};

pub(super) fn verify_instruction(
    module: &BytecodeModule,
    function: &BytecodeFunction,
    instruction: &BytecodeInstruction,
    program: Option<&BytecodeProgram>,
) -> Result<(), BytecodeVerificationError> {
    if let Some(arguments) = instruction.layout_arguments()
        && !types_in_scope(
            arguments,
            function
                .metadata
                .semantic
                .generic
                .as_ref()
                .map_or(&[], |body| body.parameters.as_slice()),
            &Default::default(),
        )
    {
        return Err(BytecodeVerificationError::InvalidOperation {
            function: function.id,
            reason: "aggregate layout argument scope",
        });
    }
    match instruction {
        BytecodeInstruction::MakeFuture {
            dst,
            function: target,
            arguments,
            future,
        } => {
            let invalid = || BytecodeVerificationError::InvalidOperation {
                function: function.id,
                reason: "invalid cold script Future contract",
            };
            let callee = module.functions.get(target.index()).ok_or_else(invalid)?;
            let Ty::NativeObject(nominal) = future else {
                return Err(invalid());
            };
            let owner = if nominal.declaration.module == module.identity {
                Some(module)
            } else {
                program.and_then(|program| {
                    program
                        .modules
                        .iter()
                        .find(|owner| owner.identity == nominal.declaration.module)
                })
            };
            let declaration = owner.and_then(|owner| {
                type_contract(&owner.identity, &owner.public_items, &nominal.declaration)
            });
            if declaration
                .is_none_or(|ty| ty.kind != TypeDefKind::NativeStorage(NativeStorageLayout::Future))
                || nominal.arguments.len() != 1
                || !nominal.associated_types.is_empty()
                || function.metadata.semantic.registers.get(&dst.index()) != Some(future)
                || !callee.metadata.effects.may_suspend
                || callee.metadata.semantic.result.as_ref() != nominal.arguments.first()
                || arguments.len() != callee.metadata.params.len()
                || (callee.metadata.semantic.generic.is_some()
                    && callee.metadata.semantic.generic != function.metadata.semantic.generic)
            {
                return Err(invalid());
            }
            expect_register_ty(function, *dst, ValueType::HeapObject, "Future destination")?;
            for (index, (argument, parameter)) in
                arguments.iter().zip(&callee.metadata.params).enumerate()
            {
                expect_register_ty(function, *argument, *parameter, "Future capture")?;
                if *parameter == ValueType::HostHandle
                    || callee
                        .metadata
                        .semantic
                        .params
                        .get(&index)
                        .is_some_and(inline_host)
                    || !callee.metadata.semantic.params.contains_key(&index)
                    || function.metadata.semantic.registers.get(&argument.index())
                        != callee.metadata.semantic.params.get(&index)
                {
                    return Err(invalid());
                }
            }
        }
        BytecodeInstruction::Await { dst, value, future } => {
            let invalid = || BytecodeVerificationError::InvalidOperation {
                function: function.id,
                reason: "invalid Future await contract",
            };
            let Ty::NativeObject(nominal) = future else {
                return Err(invalid());
            };
            let owner = if nominal.declaration.module == module.identity {
                Some(module)
            } else {
                program.and_then(|program| {
                    program
                        .modules
                        .iter()
                        .find(|owner| owner.identity == nominal.declaration.module)
                })
            };
            let declaration = owner.and_then(|owner| {
                type_contract(&owner.identity, &owner.public_items, &nominal.declaration)
            });
            if declaration
                .is_none_or(|ty| ty.kind != TypeDefKind::NativeStorage(NativeStorageLayout::Future))
                || nominal.arguments.len() != 1
                || !nominal.associated_types.is_empty()
                || !function.metadata.effects.may_suspend
                || function.metadata.semantic.registers.get(&value.index()) != Some(future)
                || function.metadata.semantic.registers.get(&dst.index())
                    != nominal.arguments.first()
            {
                return Err(invalid());
            }
            expect_register_ty(function, *value, ValueType::HeapObject, "await Future")?;
            expect_register_ty(
                function,
                *dst,
                semantic_representation(&nominal.arguments[0]),
                "await output",
            )?;
        }
        BytecodeInstruction::LoadConst { dst, constant } => {
            if !module.constants.contains(constant) {
                return Err(BytecodeVerificationError::MissingConstant {
                    function: function.id,
                });
            }
            expect_register_ty(function, *dst, constant_type(constant), "load const dst")?;
        }
        BytecodeInstruction::LoadLocal { dst, local } => {
            let local_ty = local_ty(function, *local)?;
            expect_register_ty(function, *dst, local_ty, "load local dst")?;
        }
        BytecodeInstruction::LoadModule { dst, slot } => {
            let slot_ty = module_slot_ty(module, function, *slot)?;
            expect_register_ty(function, *dst, slot_ty, "load module dst")?;
        }
        BytecodeInstruction::StoreLocal { local, src } => {
            let local_ty = local_ty(function, *local)?;
            expect_register_ty(function, *src, local_ty, "store local src")?;
        }
        BytecodeInstruction::StoreModule { slot, src } => {
            let slot_ty = module_slot_ty(module, function, *slot)?;
            expect_register_ty(function, *src, slot_ty, "store module src")?;
            if !module.module_slots[slot.index()].mutable {
                return Err(BytecodeVerificationError::InvalidOperation {
                    function: function.id,
                    reason: "store to immutable module slot",
                });
            }
        }
        BytecodeInstruction::Move { dst, src } => {
            let src_ty = register_ty(function, *src)?;
            expect_register_ty(function, *dst, src_ty, "move dst")?;
        }
        BytecodeInstruction::Unary { dst, op, operand } => {
            let op = match op {
                UnaryOp::Neg => MirUnaryOp::Neg,
                UnaryOp::Not => MirUnaryOp::Not,
            };
            let ty = contracts::unary_result(op, register_ty(function, *operand)?)
                .map_err(|error| contract_error(function, error))?;
            expect_register_ty(function, *dst, ty, "unary dst")?;
        }
        BytecodeInstruction::Binary { dst, op, lhs, rhs } => {
            let ty = contracts::binary_result(
                ir_binary_op(*op),
                register_ty(function, *lhs)?,
                register_ty(function, *rhs)?,
            )
            .map_err(|error| contract_error(function, error))?;
            expect_register_ty(function, *dst, ty, "binary dst")?;
        }
        BytecodeInstruction::Call { dst, callee, args } => {
            verify_call(module, function, *dst, callee, args, program)?;
        }
        BytecodeInstruction::BeginIteration { collection } => {
            expect_register_ty(
                function,
                *collection,
                ValueType::HeapObject,
                "iteration collection",
            )?;
        }
        BytecodeInstruction::EndIteration => {}
        BytecodeInstruction::RangeBound {
            dst,
            value,
            range,
            bound,
            ..
        } => {
            expect_register_ty(function, *dst, ValueType::HeapObject, "bound destination")?;
            expect_register_ty(function, *value, ValueType::HeapObject, "bound range")?;
            if !operations::range_bound_valid(range, bound) {
                return Err(BytecodeVerificationError::InvalidOperation {
                    function: function.id,
                    reason: "invalid range bound contract",
                });
            }
        }
        BytecodeInstruction::MakeRange {
            dst,
            start,
            end,
            ty,
        } => {
            expect_register_ty(function, *dst, ValueType::HeapObject, "range destination")?;
            if !operations::range_operands_valid(
                ty,
                start.map(|r| register_ty(function, r)).transpose()?,
                end.map(|r| register_ty(function, r)).transpose()?,
            ) {
                return Err(BytecodeVerificationError::InvalidOperation {
                    function: function.id,
                    reason: "invalid range operands",
                });
            }
        }
        BytecodeInstruction::RepeatArray {
            dst,
            element,
            value,
            count,
        } => {
            expect_register_ty(function, *dst, ValueType::HeapObject, "repeat array dst")?;
            expect_register_ty(function, *count, ValueType::U64, "repeat array count")?;
            expect_register_ty(
                function,
                *value,
                semantic_representation(element),
                "repeat array element",
            )?;
            if !element.within_wire_limits()
                || (!element.is_concrete()
                    && !function
                        .metadata
                        .semantic
                        .generic
                        .as_ref()
                        .is_some_and(|body| body.types_valid([element], &Default::default())))
            {
                return Err(BytecodeVerificationError::InvalidOperation {
                    function: function.id,
                    reason: "unresolved array element",
                });
            }
        }
        BytecodeInstruction::MakeArray {
            dst,
            element,
            elements,
        } => {
            expect_register_ty(function, *dst, ValueType::HeapObject, "array dst")?;
            if !element.within_wire_limits()
                || (!element.is_concrete()
                    && !function
                        .metadata
                        .semantic
                        .generic
                        .as_ref()
                        .is_some_and(|body| body.types_valid([element], &Default::default())))
            {
                return Err(BytecodeVerificationError::InvalidOperation {
                    function: function.id,
                    reason: "unresolved or oversized array element",
                });
            }
            for value in elements {
                expect_register_ty(
                    function,
                    *value,
                    semantic_representation(element),
                    "array element",
                )?;
            }
        }
        BytecodeInstruction::MakeTuple { dst, elements } => {
            expect_register_ty(function, *dst, ValueType::HeapObject, "tuple dst")?;
            for element in elements {
                let _ = register_ty(function, *element)?;
            }
        }
        BytecodeInstruction::MakeClosure {
            dst,
            function: target,
            captures,
        } => {
            expect_register_ty(function, *dst, ValueType::HeapObject, "closure dst")?;
            let callee = module.functions.get(target.index()).ok_or(
                BytecodeVerificationError::InvalidFunctionRef {
                    function: function.id,
                    target: *target,
                },
            )?;
            if callee.metadata.effects.may_suspend {
                return Err(BytecodeVerificationError::InvalidOperation {
                    function: function.id,
                    reason: "resume body cannot be an ordinary closure",
                });
            }
            if captures.len() > callee.metadata.params.len() {
                return Err(BytecodeVerificationError::InvalidOperation {
                    function: function.id,
                    reason: "closure capture count exceeds parameter count",
                });
            }
            if callee.metadata.semantic.generic.is_some()
                && callee.metadata.semantic.generic != function.metadata.semantic.generic
            {
                return Err(BytecodeVerificationError::InvalidOperation {
                    function: function.id,
                    reason: "closure generic environment differs from its caller",
                });
            }
            for (capture, ty) in captures.iter().zip(&callee.metadata.params) {
                expect_register_ty(function, *capture, *ty, "closure capture")?;
            }
        }
        BytecodeInstruction::MakeCell { dst, value } => {
            expect_register_ty(function, *dst, ValueType::HeapObject, "cell dst")?;
            let _ = register_ty(function, *value)?;
        }
        BytecodeInstruction::ReadCell { dst, cell } => {
            expect_register_ty(function, *cell, ValueType::HeapObject, "cell handle")?;
            let _ = register_ty(function, *dst)?;
        }
        BytecodeInstruction::WriteCell { cell, value } => {
            expect_register_ty(function, *cell, ValueType::HeapObject, "cell handle")?;
            let _ = register_ty(function, *value)?;
        }
        BytecodeInstruction::UpcastInterface {
            dst,
            value,
            source,
            target,
        } => {
            expect_register_ty(
                function,
                *dst,
                ValueType::HeapObject,
                "interface destination",
            )?;
            expect_register_ty(
                function,
                *value,
                ValueType::HeapObject,
                "interface receiver",
            )?;
            if !types_in_scope(
                [&Ty::Trait(source.clone()), &Ty::Trait(target.clone())],
                function
                    .metadata
                    .semantic
                    .generic
                    .as_ref()
                    .map_or(&[], |body| body.parameters.as_slice()),
                &Default::default(),
            ) {
                return Err(BytecodeVerificationError::InvalidInterfaceTable);
            }
        }
        BytecodeInstruction::MakeInterface {
            dst,
            value,
            module: target,
            implementation,
            arguments,
        } => {
            expect_register_ty(function, *dst, ValueType::HeapObject, "interface dst")?;
            let target_module = if let Some(program) = program {
                program
                    .modules
                    .get(target.index())
                    .ok_or(BytecodeVerificationError::InvalidInterfaceTable)?
            } else if target.index() == 0 {
                module
            } else {
                return Err(BytecodeVerificationError::InvalidInterfaceTable);
            };
            let linked = target_module
                .interface_tables
                .get(implementation.index())
                .ok_or(BytecodeVerificationError::InvalidInterfaceTable)?;
            let table = target_module
                .public_items
                .iter()
                .find_map(|item| match item {
                    PublicItem::InterfaceTable(table)
                        if table.declaration == linked.declaration =>
                    {
                        let scope = function
                            .metadata
                            .semantic
                            .generic
                            .as_ref()
                            .map_or(&[][..], |body| body.parameters.as_slice());
                        if linked.arguments.iter().all(Ty::is_concrete)
                            && linked.arguments != *arguments
                        {
                            return None;
                        }
                        table.instantiate_in(arguments, scope)
                    }
                    _ => None,
                })
                .ok_or(BytecodeVerificationError::InvalidInterfaceTable)?;
            expect_register_ty(
                function,
                *value,
                semantic_representation(&table.for_type),
                "interface receiver",
            )?;
        }
        BytecodeInstruction::Convert {
            dst,
            src,
            conversion,
        } => {
            let (input, output) =
                conversion
                    .contract()
                    .ok_or(BytecodeVerificationError::InvalidOperation {
                        function: function.id,
                        reason: "invalid numeric conversion",
                    })?;
            expect_register_ty(
                function,
                *src,
                semantic_representation(&input),
                "conversion source",
            )?;
            expect_register_ty(
                function,
                *dst,
                semantic_representation(&output),
                "conversion destination",
            )?;
        }
        BytecodeInstruction::Numeric {
            dst,
            operation,
            lhs,
            rhs,
        } => {
            let invalid = || BytecodeVerificationError::InvalidOperation {
                function: function.id,
                reason: "invalid numeric contract",
            };
            let (left, right, output) = operation.contract().ok_or_else(invalid)?;
            expect_register_ty(
                function,
                *lhs,
                semantic_representation(&left),
                "numeric lhs",
            )?;
            expect_register_ty(
                function,
                *dst,
                semantic_representation(&output),
                "numeric output",
            )?;
            match (right, rhs) {
                (Some(ty), Some(value)) => expect_register_ty(
                    function,
                    *value,
                    semantic_representation(&ty),
                    "numeric rhs",
                )?,
                (None, None) => {}
                _ => return Err(invalid()),
            }
        }
        BytecodeInstruction::Iter { dst, value, ty, op } => {
            let invalid = || BytecodeVerificationError::InvalidOperation {
                function: function.id,
                reason: "invalid iterator contract",
            };
            let (input, output) = op.contract(ty).ok_or_else(invalid)?;
            match (input, value) {
                (Some(ty), Some(value)) => {
                    expect_register_ty(function, *value, ty, "iterator input")?
                }
                (None, None) => {}
                _ => return Err(invalid()),
            }
            expect_register_ty(function, *dst, output, "iterator result")?;
        }

        BytecodeInstruction::MakeEnum {
            dst,
            enumeration,
            arguments,
            variant,
            fields,
        } => {
            expect_register_ty(function, *dst, ValueType::HeapObject, "enum dst")?;
            let invalid = || BytecodeVerificationError::InvalidOperation {
                function: function.id,
                reason: "enum initializer layout or payload count",
            };
            let layout = module
                .enumerations
                .get(enumeration.index())
                .and_then(|layout| layout.apply(arguments, &Default::default()))
                .and_then(|layout| layout.variants.get(*variant as usize).cloned())
                .ok_or_else(invalid)?;
            if fields.len() != layout.payload.len() {
                return Err(invalid());
            }
            for (register, ty) in fields.iter().zip(&layout.payload) {
                expect_register_ty(
                    function,
                    *register,
                    semantic_representation(ty),
                    "enum payload",
                )?;
            }
        }
        BytecodeInstruction::TestEnumVariant {
            dst,
            value,
            enumeration,
            arguments,
            variant,
        } => {
            expect_register_ty(function, *dst, ValueType::Bool, "enum pattern result")?;
            expect_register_ty(
                function,
                *value,
                ValueType::HeapObject,
                "enum pattern value",
            )?;
            module
                .enumerations
                .get(enumeration.index())
                .and_then(|layout| layout.apply(arguments, &Default::default()))
                .and_then(|layout| layout.variants.get(*variant as usize).cloned())
                .ok_or(BytecodeVerificationError::InvalidOperation {
                    function: function.id,
                    reason: "enum pattern variant",
                })?;
        }
        BytecodeInstruction::ReadEnumPayload {
            dst,
            value,
            enumeration,
            arguments,
            variant,
            index,
        } => {
            expect_register_ty(
                function,
                *value,
                ValueType::HeapObject,
                "enum pattern value",
            )?;
            let ty = module
                .enumerations
                .get(enumeration.index())
                .and_then(|layout| layout.apply(arguments, &Default::default()))
                .and_then(|layout| layout.variants.get(*variant as usize).cloned())
                .and_then(|variant| variant.payload.get(*index as usize).cloned())
                .ok_or(BytecodeVerificationError::InvalidOperation {
                    function: function.id,
                    reason: "enum pattern payload",
                })?;
            expect_register_ty(
                function,
                *dst,
                semantic_representation(&ty),
                "enum pattern payload dst",
            )?;
        }
        BytecodeInstruction::MakeStruct {
            dst,
            structure,
            arguments,
            fields,
        } => {
            expect_register_ty(function, *dst, ValueType::HeapObject, "struct dst")?;
            let layout = module
                .structures
                .get(structure.index())
                .and_then(|layout| layout.apply(arguments, &Default::default()))
                .ok_or(BytecodeVerificationError::InvalidStructId {
                    function: function.id,
                    structure: *structure,
                })?;
            if fields.len() != layout.fields.len() {
                return Err(BytecodeVerificationError::InvalidOperation {
                    function: function.id,
                    reason: "struct initializer field count",
                });
            }
            for (value, field) in fields.iter().zip(&layout.fields) {
                expect_register_ty(
                    function,
                    *value,
                    semantic_representation(&field.ty),
                    "struct field initializer",
                )?;
            }
        }
        BytecodeInstruction::ReadAggregateField { dst, base, field } => {
            let field = field_layout(module, function, field)?;
            expect_register_ty(
                function,
                *dst,
                semantic_representation(&field.ty),
                "aggregate field dst",
            )?;
            expect_register_ty(function, *base, ValueType::HeapObject, "field base")?;
        }
        BytecodeInstruction::WriteAggregateField { base, field, value } => {
            let field = field_layout(module, function, field)?;
            if !field.mutable {
                return Err(BytecodeVerificationError::InvalidOperation {
                    function: function.id,
                    reason: "write to read-only field",
                });
            }
            expect_register_ty(function, *base, ValueType::HeapObject, "field base")?;
            expect_register_ty(
                function,
                *value,
                semantic_representation(&field.ty),
                "aggregate field value",
            )?;
        }
        BytecodeInstruction::ReadAggregateIndex { dst, base, index } => {
            let _ = register_ty(function, *dst)?;
            expect_register_ty(function, *base, ValueType::HeapObject, "index base")?;
            let _ = register_ty(function, *index)?;
        }
        BytecodeInstruction::WriteAggregateIndex { base, index, value } => {
            expect_register_ty(function, *base, ValueType::HeapObject, "index base")?;
            let _ = register_ty(function, *index)?;
            let _ = register_ty(function, *value)?;
        }
        BytecodeInstruction::ReadPath {
            dst,
            root_or_view,
            path,
            dynamic_args,
        } => {
            let path = path_record(module, function, *path)?;
            expect_register_ty(function, *dst, path.result_ty, "path read dst")?;
            expect_register_ty(function, *root_or_view, path.root_ty, "path root")?;
            verify_dynamic_path_args(function, dynamic_args)?;
        }
        BytecodeInstruction::SetPath {
            root_or_view,
            path,
            dynamic_args,
            value,
        } => {
            let path = path_record(module, function, *path)?;
            if path.read_only {
                return Err(BytecodeVerificationError::ReadOnlyPath {
                    function: function.id,
                    path: path.id,
                });
            }
            expect_register_ty(function, *root_or_view, path.root_ty, "path root")?;
            expect_register_ty(function, *value, path.result_ty, "path set value")?;
            verify_dynamic_path_args(function, dynamic_args)?;
        }
        BytecodeInstruction::ModifyPath {
            dst,
            root_or_view,
            path,
            dynamic_args,
            value,
            op,
        } => {
            let path = path_record(module, function, *path)?;
            if path.read_only {
                return Err(BytecodeVerificationError::ReadOnlyPath {
                    function: function.id,
                    path: path.id,
                });
            }
            expect_register_ty(function, *root_or_view, path.root_ty, "path root")?;
            expect_register_ty(function, *value, path.result_ty, "path modify value")?;
            let result =
                contracts::binary_result(ir_binary_op(*op), path.result_ty, path.result_ty)
                    .map_err(|error| contract_error(function, error))?;
            if result != path.result_ty {
                return Err(BytecodeVerificationError::TypeMismatch {
                    function: function.id,
                    context: "path modification result",
                    expected: path.result_ty,
                    found: result,
                });
            }
            if let Some(dst) = dst {
                expect_register_ty(function, *dst, path.result_ty, "path modify dst")?;
            }
            verify_dynamic_path_args(function, dynamic_args)?;
        }
        BytecodeInstruction::MakePathView {
            dst,
            root_or_view,
            path,
            dynamic_args,
        } => {
            let path = path_record(module, function, *path)?;
            expect_register_ty(function, *dst, ValueType::HostHandle, "path view dst")?;
            expect_register_ty(function, *root_or_view, path.root_ty, "path root")?;
            verify_dynamic_path_args(function, dynamic_args)?;
        }
        BytecodeInstruction::Jump { target } => verify_jump(function, *target)?,
        BytecodeInstruction::Branch {
            cond,
            then_target,
            else_target,
        } => {
            expect_register_ty(function, *cond, ValueType::Bool, "branch condition")?;
            verify_jump(function, *then_target)?;
            verify_jump(function, *else_target)?;
        }
        BytecodeInstruction::Return(value) => {
            if function.metadata.return_type == ValueType::Never {
                return Err(BytecodeVerificationError::InvalidOperation {
                    function: function.id,
                    reason: "Never function cannot return",
                });
            }
            let found = value
                .map(|value| register_ty(function, value))
                .transpose()?;
            let found = found.unwrap_or(ValueType::Unit);
            if found != function.metadata.return_type {
                return Err(BytecodeVerificationError::TypeMismatch {
                    function: function.id,
                    context: "return value",
                    expected: function.metadata.return_type,
                    found,
                });
            }
        }
        BytecodeInstruction::Unreachable => {}
    }
    Ok(())
}

pub(super) fn verify_call(
    module: &BytecodeModule,
    function: &BytecodeFunction,
    dst: Option<Register>,
    callee: &CallTarget,
    args: &[Register],
    program: Option<&BytecodeProgram>,
) -> Result<(), BytecodeVerificationError> {
    match callee {
        CallTarget::Shared {
            module: owner,
            target,
            contract,
        } => {
            let invalid = || BytecodeVerificationError::InvalidOperation {
                function: function.id,
                reason: "invalid shared call environment",
            };
            let owner = program
                .and_then(|program| program.modules.get(owner.index()))
                .or_else(|| (program.is_none() && owner.index() == 0).then_some(module))
                .ok_or_else(invalid)?;
            let entry = shared::entry(owner, *target).ok_or_else(invalid)?;
            if entry.identity != &contract.instance
                || entry.implementation != contract.implementation
                || !contract.structurally_valid(
                    function
                        .metadata
                        .semantic
                        .generic
                        .as_ref()
                        .map_or(&[], |body| body.parameters.as_slice()),
                    &Default::default(),
                )
                || contract.arguments.len() != entry.body.parameters.len()
                || contract.signature.params.len() != args.len()
            {
                return Err(invalid());
            }
            for (arg, ty) in args.iter().zip(&contract.signature.params) {
                expect_register_ty(
                    function,
                    *arg,
                    semantic_representation(ty),
                    "shared call argument",
                )?;
            }
            verify_call_dst(
                function,
                dst,
                semantic_representation(&contract.signature.result),
            )?;
        }
        CallTarget::ModuleFunction {
            module: target_module,
            function: target,
        } => {
            let target_module = program
                .and_then(|program| program.modules.get(target_module.index()))
                .ok_or(BytecodeVerificationError::InvalidProgramGraph)?;
            verify_call(
                target_module,
                function,
                dst,
                &CallTarget::Function(*target),
                args,
                None,
            )?;
        }
        CallTarget::Function(target) => {
            if !function_ref_exists(module, *target) {
                return Err(BytecodeVerificationError::InvalidFunctionRef {
                    function: function.id,
                    target: *target,
                });
            }
            let record = &module.function_table[target.index()];
            if record.effects.may_suspend {
                return Err(BytecodeVerificationError::InvalidOperation {
                    function: function.id,
                    reason: "resume body cannot be called synchronously",
                });
            }
            if module.functions[target.index()]
                .metadata
                .semantic
                .generic
                .is_some()
            {
                return Err(BytecodeVerificationError::InvalidOperation {
                    function: function.id,
                    reason: "shared function requires a checked generic call environment",
                });
            }
            if record.params.len() != args.len() {
                return Err(BytecodeVerificationError::ArityMismatch {
                    function: function.id,
                    target: *target,
                    expected: record.params.len(),
                    found: args.len(),
                });
            }
            for (arg, expected) in args.iter().zip(&record.params) {
                expect_register_ty(function, *arg, *expected, "call argument")?;
            }
            verify_call_dst(function, dst, record.return_type)?;
        }
        CallTarget::InterfaceMethod {
            module: owner_slot,
            contract,
        } => {
            let owner = if let Some(program) = program {
                program.modules.get(owner_slot.index())
            } else if owner_slot.index() == 0 {
                Some(module)
            } else {
                None
            }
            .ok_or(BytecodeVerificationError::InvalidProgramGraph)?;
            let signature = contract
                .signature_in(
                    &owner.identity,
                    &owner.public_items,
                    &owner.trait_contracts,
                    &Default::default(),
                )
                .map_err(|_| BytecodeVerificationError::InvalidOperation {
                    function: function.id,
                    reason: "invalid linked interface method",
                })?;
            if !signature.types_valid(
                function
                    .metadata
                    .semantic
                    .generic
                    .as_ref()
                    .map_or(&[], |body| body.parameters.as_slice()),
                &Default::default(),
            ) {
                return Err(BytecodeVerificationError::InvalidOperation {
                    function: function.id,
                    reason: "unbound interface method application",
                });
            }
            let params: Vec<_> = signature
                .params
                .iter()
                .map(semantic_representation)
                .collect();
            let return_type = semantic_representation(&signature.result);
            if args.len() != params.len() {
                return Err(BytecodeVerificationError::InvalidOperation {
                    function: function.id,
                    reason: "interface method arity mismatch",
                });
            }
            for (arg, expected) in args.iter().zip(params) {
                expect_register_ty(function, *arg, expected, "interface method argument")?;
            }
            verify_call_dst(function, dst, return_type)?;
        }
        CallTarget::Native(import) => {
            let contract = module.native_imports.get(import.index()).ok_or(
                BytecodeVerificationError::InvalidOperation {
                    function: function.id,
                    reason: "invalid engine import index",
                },
            )?;
            if contract.generic.is_some() {
                return Err(BytecodeVerificationError::InvalidOperation {
                    function: function.id,
                    reason: "shared native requires a checked generic call environment",
                });
            }
            let args = args
                .iter()
                .map(|arg| register_ty(function, *arg))
                .collect::<Result<Vec<_>, _>>()?;
            let dst = dst.map(|dst| register_ty(function, dst)).transpose()?;
            contracts::verify_native_call(dst, contract, &args)
                .map_err(|error| contract_error(function, error))?;
        }

        CallTarget::Register(_) => {
            return Err(BytecodeVerificationError::InvalidOperation {
                function: function.id,
                reason: "dynamic register calls have no executable contract",
            });
        }
        CallTarget::ClosureRegister {
            register,
            params,
            return_type,
        } => {
            expect_register_ty(function, *register, ValueType::HeapObject, "closure callee")?;
            if args.len() != params.len() {
                return Err(BytecodeVerificationError::InvalidOperation {
                    function: function.id,
                    reason: "closure argument count mismatch",
                });
            }
            for (arg, ty) in args.iter().zip(params) {
                expect_register_ty(function, *arg, *ty, "closure argument")?;
            }
            verify_call_dst(function, dst, *return_type)?;
        }
        CallTarget::RuntimePrimitive(intrinsic) => {
            verify_standard_intrinsic_call(function, dst, *intrinsic, args)?;
        }
        CallTarget::RuntimeHelper(RuntimeHelper::DynamicCall) => {
            return Err(BytecodeVerificationError::InvalidOperation {
                function: function.id,
                reason: "dynamic invocation has no executable contract",
            });
        }
        CallTarget::RuntimeHelper(helper) => {
            let kind = match helper {
                RuntimeHelper::ReflectTypeOf => RuntimeHelperKind::TypeOf,
                RuntimeHelper::ReflectGetField(_) => RuntimeHelperKind::GetField,
                RuntimeHelper::ReflectSetField(_) => RuntimeHelperKind::SetField,
                RuntimeHelper::ReflectSetIndex => RuntimeHelperKind::SetIndex,
                RuntimeHelper::DynamicCall => unreachable!(),
            };
            let args = args
                .iter()
                .map(|arg| register_ty(function, *arg))
                .collect::<Result<Vec<_>, _>>()?;
            let dst = dst.map(|dst| register_ty(function, dst)).transpose()?;
            contracts::verify_runtime_helper_call(dst, kind, &args)
                .map_err(|error| contract_error(function, error))?;
        }
    }
    Ok(())
}

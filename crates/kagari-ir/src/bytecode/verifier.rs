use crate::{
    bytecode::{
        BinaryOp, BytecodeFunction, BytecodeInstruction, BytecodeModule, CallTarget,
        ConstantOperand, FieldId, FunctionRef, JumpTarget, LocalSlot, ModuleSlot, PathId, Register,
        UnaryOp,
    },
    module::ValueType,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BytecodeVerificationError {
    InvalidModuleInit(FunctionRef),
    FunctionTableLengthMismatch {
        functions: usize,
        table: usize,
    },
    FunctionRecordMismatch {
        function: FunctionRef,
    },
    MetadataCountMismatch {
        function: FunctionRef,
        layout: &'static str,
        expected: usize,
        found: usize,
    },
    InvalidRegister {
        function: FunctionRef,
        register: Register,
    },
    InvalidLocal {
        function: FunctionRef,
        local: LocalSlot,
    },
    InvalidModuleSlot {
        function: FunctionRef,
        slot: ModuleSlot,
    },
    InvalidFieldId {
        function: FunctionRef,
        field: FieldId,
    },
    InvalidPathId {
        function: FunctionRef,
        path: PathId,
    },
    ReadOnlyPath {
        function: FunctionRef,
        path: PathId,
    },
    InvalidFunctionRef {
        function: FunctionRef,
        target: FunctionRef,
    },
    MissingConstant {
        function: FunctionRef,
    },
    MissingType {
        function: FunctionRef,
        ty: ValueType,
    },
    InvalidJumpTarget {
        function: FunctionRef,
        target: JumpTarget,
    },
    TypeMismatch {
        function: FunctionRef,
        context: &'static str,
        expected: ValueType,
        found: ValueType,
    },
    ArityMismatch {
        function: FunctionRef,
        target: FunctionRef,
        expected: usize,
        found: usize,
    },
}

pub fn verify_module(module: &BytecodeModule) -> Result<(), BytecodeVerificationError> {
    if module.function_table.len() != module.functions.len() {
        return Err(BytecodeVerificationError::FunctionTableLengthMismatch {
            functions: module.functions.len(),
            table: module.function_table.len(),
        });
    }
    if let Some(init) = module.module_init
        && !function_ref_exists(module, init)
    {
        return Err(BytecodeVerificationError::InvalidModuleInit(init));
    }
    for (index, function) in module.functions.iter().enumerate() {
        let expected_ref = FunctionRef::new(index);
        if function.id != expected_ref {
            return Err(BytecodeVerificationError::FunctionRecordMismatch {
                function: function.id,
            });
        }
        let Some(record) = module.function_table.get(index) else {
            unreachable!("function table length was already checked");
        };
        if record.id != function.id
            || record.name != function.name
            || record.params != function.metadata.params
            || record.return_type != function.metadata.return_type
            || record.effects != function.metadata.effects
        {
            return Err(BytecodeVerificationError::FunctionRecordMismatch {
                function: function.id,
            });
        }
        verify_function(module, function)?;
    }
    Ok(())
}

fn verify_function(
    module: &BytecodeModule,
    function: &BytecodeFunction,
) -> Result<(), BytecodeVerificationError> {
    verify_metadata_counts(function)?;
    verify_metadata_types(module, function)?;
    verify_debug_metadata(function)?;
    for target in &function.metadata.control_flow_targets {
        verify_jump(function, *target)?;
    }
    for instruction in &function.instructions {
        verify_instruction(module, function, instruction)?;
    }
    Ok(())
}

fn verify_metadata_types(
    module: &BytecodeModule,
    function: &BytecodeFunction,
) -> Result<(), BytecodeVerificationError> {
    for ty in std::iter::once(&function.metadata.return_type)
        .chain(&function.metadata.params)
        .chain(&function.metadata.locals)
        .chain(&function.metadata.registers)
    {
        if !module.types.contains(ty) {
            return Err(BytecodeVerificationError::MissingType {
                function: function.id,
                ty: *ty,
            });
        }
    }
    Ok(())
}

fn verify_metadata_counts(function: &BytecodeFunction) -> Result<(), BytecodeVerificationError> {
    let checks = [
        (
            "params",
            usize::from(function.parameter_count),
            function.metadata.params.len(),
        ),
        (
            "locals",
            usize::from(function.local_count),
            function.metadata.locals.len(),
        ),
        (
            "registers",
            usize::from(function.register_count),
            function.metadata.registers.len(),
        ),
    ];
    for (layout, expected, found) in checks {
        if expected != found {
            return Err(BytecodeVerificationError::MetadataCountMismatch {
                function: function.id,
                layout,
                expected,
                found,
            });
        }
    }
    if usize::from(function.parameter_count) > usize::from(function.local_count) {
        return Err(BytecodeVerificationError::MetadataCountMismatch {
            function: function.id,
            layout: "params",
            expected: usize::from(function.local_count),
            found: usize::from(function.parameter_count),
        });
    }
    for (index, param_ty) in function.metadata.params.iter().enumerate() {
        let local_ty = function.metadata.locals[index];
        if *param_ty != local_ty {
            return Err(BytecodeVerificationError::TypeMismatch {
                function: function.id,
                context: "parameter local layout",
                expected: *param_ty,
                found: local_ty,
            });
        }
    }
    Ok(())
}

fn verify_debug_metadata(function: &BytecodeFunction) -> Result<(), BytecodeVerificationError> {
    for source_span in &function.metadata.debug.source_spans {
        verify_instruction_offset(function, source_span.instruction_offset)?;
    }
    for line in &function.metadata.debug.line_table {
        verify_instruction_offset(function, line.instruction_offset)?;
    }
    for point in &function.metadata.debug.safe_debug_points {
        verify_instruction_offset(function, point.instruction_offset)?;
    }
    for range in &function.metadata.debug.local_live_ranges {
        let _ = local_ty(function, range.local)?;
        if range.start > range.end || range.end > function.instructions.len() {
            return Err(BytecodeVerificationError::InvalidJumpTarget {
                function: function.id,
                target: JumpTarget::new(range.end),
            });
        }
    }
    Ok(())
}

fn verify_instruction_offset(
    function: &BytecodeFunction,
    instruction_offset: usize,
) -> Result<(), BytecodeVerificationError> {
    if instruction_offset < function.instructions.len() {
        Ok(())
    } else {
        Err(BytecodeVerificationError::InvalidJumpTarget {
            function: function.id,
            target: JumpTarget::new(instruction_offset),
        })
    }
}

fn verify_instruction(
    module: &BytecodeModule,
    function: &BytecodeFunction,
    instruction: &BytecodeInstruction,
) -> Result<(), BytecodeVerificationError> {
    match instruction {
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
        }
        BytecodeInstruction::Move { dst, src } => {
            let src_ty = register_ty(function, *src)?;
            expect_register_ty(function, *dst, src_ty, "move dst")?;
        }
        BytecodeInstruction::Unary { dst, op, operand } => {
            let operand_ty = register_ty(function, *operand)?;
            let expected = match op {
                UnaryOp::Neg => operand_ty,
                UnaryOp::Not => ValueType::Bool,
            };
            if *op == UnaryOp::Not && operand_ty != ValueType::Bool {
                return Err(BytecodeVerificationError::TypeMismatch {
                    function: function.id,
                    context: "unary operand",
                    expected: ValueType::Bool,
                    found: operand_ty,
                });
            }
            expect_register_ty(function, *dst, expected, "unary dst")?;
        }
        BytecodeInstruction::Binary { dst, op, lhs, rhs } => {
            let lhs_ty = register_ty(function, *lhs)?;
            expect_register_ty(function, *rhs, lhs_ty, "binary rhs")?;
            let dst_ty = match op {
                BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div => lhs_ty,
                BinaryOp::Eq
                | BinaryOp::NotEq
                | BinaryOp::Lt
                | BinaryOp::Gt
                | BinaryOp::Le
                | BinaryOp::Ge => ValueType::Bool,
            };
            expect_register_ty(function, *dst, dst_ty, "binary dst")?;
        }
        BytecodeInstruction::Call { dst, callee, args } => {
            verify_call(module, function, *dst, callee, args)?;
        }
        BytecodeInstruction::MakeTuple { dst, elements }
        | BytecodeInstruction::MakeArray { dst, elements } => {
            expect_register_ty(function, *dst, ValueType::HeapObject, "aggregate dst")?;
            for element in elements {
                let _ = register_ty(function, *element)?;
            }
        }
        BytecodeInstruction::MakeStruct { dst, fields, .. } => {
            expect_register_ty(function, *dst, ValueType::HeapObject, "struct dst")?;
            for field in fields {
                let _ = register_ty(function, field.value)?;
            }
        }
        BytecodeInstruction::ReadAggregateField { dst, base, field } => {
            let field = field_record(module, function, *field)?;
            expect_register_ty(function, *dst, field.ty, "aggregate field dst")?;
            expect_register_ty(function, *base, ValueType::HeapObject, "field base")?;
        }
        BytecodeInstruction::WriteAggregateField { base, field, value } => {
            let field = field_record(module, function, *field)?;
            expect_register_ty(function, *base, ValueType::HeapObject, "field base")?;
            expect_register_ty(function, *value, field.ty, "aggregate field value")?;
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
            ..
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
            expect_register_ty(function, *dst, ValueType::HeapObject, "path view dst")?;
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

fn verify_call(
    module: &BytecodeModule,
    function: &BytecodeFunction,
    dst: Option<Register>,
    callee: &CallTarget,
    args: &[Register],
) -> Result<(), BytecodeVerificationError> {
    match callee {
        CallTarget::Function(target) => {
            if !function_ref_exists(module, *target) {
                return Err(BytecodeVerificationError::InvalidFunctionRef {
                    function: function.id,
                    target: *target,
                });
            }
            let record = &module.function_table[target.index()];
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
        CallTarget::Register(register) => {
            let _ = register_ty(function, *register)?;
            for arg in args {
                let _ = register_ty(function, *arg)?;
            }
        }
        CallTarget::BuiltinMethod(_) | CallTarget::RuntimeHelper(_) => {
            for arg in args {
                let _ = register_ty(function, *arg)?;
            }
            if let Some(dst) = dst {
                let _ = register_ty(function, dst)?;
            }
        }
    }
    Ok(())
}

fn verify_call_dst(
    function: &BytecodeFunction,
    dst: Option<Register>,
    return_type: ValueType,
) -> Result<(), BytecodeVerificationError> {
    match (dst, return_type) {
        (None, ValueType::Unit) => Ok(()),
        (Some(dst), ty) => expect_register_ty(function, dst, ty, "call dst"),
        (None, ty) => Err(BytecodeVerificationError::TypeMismatch {
            function: function.id,
            context: "call dst",
            expected: ty,
            found: ValueType::Unit,
        }),
    }
}

fn function_ref_exists(module: &BytecodeModule, target: FunctionRef) -> bool {
    target.index() < module.functions.len() && target.index() < module.function_table.len()
}

fn field_record<'a>(
    module: &'a BytecodeModule,
    function: &BytecodeFunction,
    field: FieldId,
) -> Result<&'a crate::bytecode::FieldRecord, BytecodeVerificationError> {
    let Some(record) = module.fields.get(field.index()) else {
        return Err(BytecodeVerificationError::InvalidFieldId {
            function: function.id,
            field,
        });
    };
    if record.id == field {
        Ok(record)
    } else {
        Err(BytecodeVerificationError::InvalidFieldId {
            function: function.id,
            field,
        })
    }
}

fn path_record<'a>(
    module: &'a BytecodeModule,
    function: &BytecodeFunction,
    path: PathId,
) -> Result<&'a crate::bytecode::PathRecord, BytecodeVerificationError> {
    let Some(record) = module.paths.get(path.index()) else {
        return Err(BytecodeVerificationError::InvalidPathId {
            function: function.id,
            path,
        });
    };
    if record.id == path {
        Ok(record)
    } else {
        Err(BytecodeVerificationError::InvalidPathId {
            function: function.id,
            path,
        })
    }
}

fn verify_dynamic_path_args(
    function: &BytecodeFunction,
    dynamic_args: &[Register],
) -> Result<(), BytecodeVerificationError> {
    for arg in dynamic_args {
        let _ = register_ty(function, *arg)?;
    }
    Ok(())
}

fn verify_jump(
    function: &BytecodeFunction,
    target: JumpTarget,
) -> Result<(), BytecodeVerificationError> {
    if target.index() < function.instructions.len() {
        Ok(())
    } else {
        Err(BytecodeVerificationError::InvalidJumpTarget {
            function: function.id,
            target,
        })
    }
}

fn register_ty(
    function: &BytecodeFunction,
    register: Register,
) -> Result<ValueType, BytecodeVerificationError> {
    function
        .metadata
        .registers
        .get(register.index())
        .copied()
        .ok_or(BytecodeVerificationError::InvalidRegister {
            function: function.id,
            register,
        })
}

fn expect_register_ty(
    function: &BytecodeFunction,
    register: Register,
    expected: ValueType,
    context: &'static str,
) -> Result<(), BytecodeVerificationError> {
    let found = register_ty(function, register)?;
    if found == expected {
        Ok(())
    } else {
        Err(BytecodeVerificationError::TypeMismatch {
            function: function.id,
            context,
            expected,
            found,
        })
    }
}

fn local_ty(
    function: &BytecodeFunction,
    local: LocalSlot,
) -> Result<ValueType, BytecodeVerificationError> {
    function.metadata.locals.get(local.index()).copied().ok_or(
        BytecodeVerificationError::InvalidLocal {
            function: function.id,
            local,
        },
    )
}

fn module_slot_ty(
    module: &BytecodeModule,
    function: &BytecodeFunction,
    slot: ModuleSlot,
) -> Result<ValueType, BytecodeVerificationError> {
    module
        .module_slots
        .get(slot.index())
        .map(|slot| slot.ty)
        .ok_or(BytecodeVerificationError::InvalidModuleSlot {
            function: function.id,
            slot,
        })
}

fn constant_type(constant: &ConstantOperand) -> ValueType {
    match constant {
        ConstantOperand::Unit => ValueType::Unit,
        ConstantOperand::Bool(_) => ValueType::Bool,
        ConstantOperand::I32(_) => ValueType::I32,
        ConstantOperand::F32(_) => ValueType::F32,
        ConstantOperand::Str(_) => ValueType::Str,
    }
}

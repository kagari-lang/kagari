use crate::{
    access,
    instruction::{
        BinaryOp, BytecodeInstruction, CallTarget, ConstantOperand, FieldRef, JumpTarget,
        LocalSlot, ModuleSlot, NativeImportId, PathId, Register, StructId,
    },
    module::{
        BytecodeFunction, BytecodeModule, CallableTarget, InterfaceMethodSlot, PathRecord,
        RootSlotLayout,
    },
    program::BytecodeProgram,
    trait_bounds,
    verifier::operation::verify_instruction,
};
use kagari_abi::representation::ValueType;
use kagari_common::identity::DefinitionKind;
use kagari_contract::{
    contracts,
    contracts::ContractError,
    host,
    ids::FunctionRef,
    layout,
    layout::StructFieldLayout,
    operations::BinaryOp as MirBinaryOp,
    standard::RuntimePrimitive,
    types::{InterfaceTable, PublicItem, inherent::native_signatures_match, verify},
};
use kagari_types::{
    callable::CallableImplementation,
    declaration::verify::{concrete_type_valid, validate_native_declarations},
    ty::Ty,
};
use std::{collections::HashSet, iter};

mod operation;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BytecodeVerificationError {
    #[error("invalid executable module graph")]
    InvalidProgramGraph,
    #[error("invalid struct layouts")]
    InvalidStructLayout,
    #[error("invalid enum layouts")]
    InvalidEnumLayout,
    #[error("invalid public ABI")]
    InvalidPublicAbi,
    #[error("invalid executable interface table")]
    InvalidInterfaceTable,
    #[error("invalid host path layout")]
    InvalidPathLayout,
    #[error("invalid struct {structure:?} in {function:?}")]
    InvalidStructId {
        function: FunctionRef,
        structure: StructId,
    },
    #[error("invalid host interface: {0}")]
    InvalidHostInterface(String),
    #[error("invalid host import {import:?} in {function:?}")]
    InvalidHostImport {
        function: FunctionRef,
        import: NativeImportId,
    },
    #[error("invalid operation in {function:?}: {reason}")]
    InvalidOperation {
        function: FunctionRef,
        reason: &'static str,
    },
    #[error("function table length mismatch: {functions} functions, {table} table records")]
    FunctionTableLengthMismatch { functions: usize, table: usize },
    #[error("function record mismatch for {function:?}")]
    FunctionRecordMismatch { function: FunctionRef },
    #[error("invalid function identity for {function:?}")]
    InvalidFunctionIdentity { function: FunctionRef },
    #[error("invalid GC root layout for {function:?}")]
    InvalidRootLayout { function: FunctionRef },
    #[error("metadata count mismatch in {function:?} {layout}: expected {expected}, found {found}")]
    MetadataCountMismatch {
        function: FunctionRef,
        layout: &'static str,
        expected: usize,
        found: usize,
    },
    #[error("invalid register {register:?} in {function:?}")]
    InvalidRegister {
        function: FunctionRef,
        register: Register,
    },
    #[error("invalid local {local:?} in {function:?}")]
    InvalidLocal {
        function: FunctionRef,
        local: LocalSlot,
    },
    #[error("invalid module slot {slot:?} in {function:?}")]
    InvalidModuleSlot {
        function: FunctionRef,
        slot: ModuleSlot,
    },
    #[error("invalid field id {field:?} in {function:?}")]
    InvalidFieldReference {
        function: FunctionRef,
        field: FieldRef,
    },
    #[error("invalid path id {path:?} in {function:?}")]
    InvalidPathId { function: FunctionRef, path: PathId },
    #[error("write to read-only path {path:?} in {function:?}")]
    ReadOnlyPath { function: FunctionRef, path: PathId },
    #[error("invalid call target {target:?} referenced from {function:?}")]
    InvalidFunctionRef {
        function: FunctionRef,
        target: FunctionRef,
    },
    #[error("instruction in {function:?} references a missing constant")]
    MissingConstant { function: FunctionRef },
    #[error("metadata in {function:?} references missing type {ty:?}")]
    MissingType {
        function: FunctionRef,
        ty: ValueType,
    },
    #[error("invalid jump target {target:?} in {function:?}")]
    InvalidJumpTarget {
        function: FunctionRef,
        target: JumpTarget,
    },
    #[error(
        "bytecode type mismatch in {function:?} {context}: expected {expected:?}, found {found:?}"
    )]
    TypeMismatch {
        function: FunctionRef,
        context: &'static str,
        expected: ValueType,
        found: ValueType,
    },
    #[error(
        "call arity mismatch in {function:?} to {target:?}: expected {expected}, found {found}"
    )]
    ArityMismatch {
        function: FunctionRef,
        target: FunctionRef,
        expected: usize,
        found: usize,
    },
    #[error("standard intrinsic signature mismatch in {function:?} for {intrinsic:?}: {reason}")]
    RuntimePrimitiveSignatureMismatch {
        function: FunctionRef,
        intrinsic: RuntimePrimitive,
        reason: &'static str,
    },
}

impl BytecodeVerificationError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidProgramGraph => "KG_BYTECODE_INVALID_PROGRAM_GRAPH",
            Self::InvalidStructLayout => "KG_BYTECODE_INVALID_STRUCT_LAYOUT",
            Self::InvalidEnumLayout => "KG_BYTECODE_INVALID_ENUM_LAYOUT",
            Self::InvalidPublicAbi => "KG_BYTECODE_INVALID_PUBLIC_ABI",
            Self::InvalidInterfaceTable => "KG_BYTECODE_INVALID_INTERFACE_TABLE",
            Self::InvalidPathLayout => "KG_BYTECODE_INVALID_PATH_LAYOUT",
            Self::InvalidStructId { .. } => "KG_BYTECODE_INVALID_STRUCT_ID",
            Self::InvalidHostInterface(_) => "KG_BYTECODE_INVALID_HOST_INTERFACE",
            Self::InvalidHostImport { .. } => "KG_BYTECODE_INVALID_HOST_IMPORT",
            Self::InvalidOperation { .. } => "KG_BYTECODE_INVALID_OPERATION",
            Self::FunctionTableLengthMismatch { .. } => {
                "KG_BYTECODE_FUNCTION_TABLE_LENGTH_MISMATCH"
            }
            Self::FunctionRecordMismatch { .. } => "KG_BYTECODE_FUNCTION_RECORD_MISMATCH",
            Self::InvalidFunctionIdentity { .. } => "KG_BYTECODE_INVALID_FUNCTION_IDENTITY",
            Self::InvalidRootLayout { .. } => "KG_BYTECODE_INVALID_ROOT_LAYOUT",
            Self::MetadataCountMismatch { .. } => "KG_BYTECODE_METADATA_COUNT_MISMATCH",
            Self::InvalidRegister { .. } => "KG_BYTECODE_INVALID_REGISTER",
            Self::InvalidLocal { .. } => "KG_BYTECODE_INVALID_LOCAL",
            Self::InvalidModuleSlot { .. } => "KG_BYTECODE_INVALID_MODULE_SLOT",
            Self::InvalidFieldReference { .. } => "KG_BYTECODE_INVALID_FIELD_REFERENCE",
            Self::InvalidPathId { .. } => "KG_BYTECODE_INVALID_PATH_ID",
            Self::ReadOnlyPath { .. } => "KG_BYTECODE_READ_ONLY_PATH",
            Self::InvalidFunctionRef { .. } => "KG_BYTECODE_INVALID_FUNCTION_REF",
            Self::MissingConstant { .. } => "KG_BYTECODE_MISSING_CONSTANT",
            Self::MissingType { .. } => "KG_BYTECODE_MISSING_TYPE",
            Self::InvalidJumpTarget { .. } => "KG_BYTECODE_INVALID_JUMP_TARGET",
            Self::TypeMismatch { .. } => "KG_BYTECODE_TYPE_MISMATCH",
            Self::ArityMismatch { .. } => "KG_BYTECODE_ARITY_MISMATCH",
            Self::RuntimePrimitiveSignatureMismatch { .. } => {
                "KG_BYTECODE_STANDARD_INTRINSIC_SIGNATURE_MISMATCH"
            }
        }
    }
}

pub fn verify_module(module: &BytecodeModule) -> Result<(), BytecodeVerificationError> {
    if !module.dependencies.is_empty() {
        return Err(BytecodeVerificationError::InvalidProgramGraph);
    }
    verify_module_with_program(module, None)?;
    trait_bounds::verify_trait_bounds(module, &[module], None)?;
    Ok(())
}

pub(super) fn verify_module_with_program(
    module: &BytecodeModule,
    program: Option<&BytecodeProgram>,
) -> Result<(), BytecodeVerificationError> {
    if module
        .paths
        .iter()
        .enumerate()
        .any(|(index, path)| path.id.index() != index || path.root_ty != ValueType::HostHandle)
    {
        return Err(BytecodeVerificationError::InvalidPathLayout);
    }
    verify::validate(&module.public_items, &module.identity, &Default::default())
        .map_err(|_| BytecodeVerificationError::InvalidPublicAbi)?;
    validate_native_declarations(
        &module.native_declarations,
        &module.identity,
        &Default::default(),
    )
    .map_err(|_| BytecodeVerificationError::InvalidPublicAbi)?;
    if !native_signatures_match(&module.public_items, &module.native_declarations) {
        return Err(BytecodeVerificationError::InvalidPublicAbi);
    }
    verify::validate_trait_contracts(
        &module.trait_contracts,
        &module.public_items,
        &module.identity,
        &Default::default(),
    )
    .map_err(|_| BytecodeVerificationError::InvalidPublicAbi)?;
    layout::validate_layouts(&module.structures, &Default::default())
        .map_err(|_| BytecodeVerificationError::InvalidStructLayout)?;
    if !layout::struct_abi_matches(
        &module.structures,
        &module.identity,
        &module.public_items,
        &Default::default(),
    )
    .expect("bytecode verification uses an uncancelled token")
    {
        return Err(BytecodeVerificationError::InvalidStructLayout);
    }
    if !layout::enum_abi_matches(
        &module.enumerations,
        &module.identity,
        &module.public_items,
        &Default::default(),
    )
    .expect("bytecode verification uses an uncancelled token")
    {
        return Err(BytecodeVerificationError::InvalidEnumLayout);
    }
    layout::validate_enum_layouts(
        &module.enumerations,
        &module.structures,
        &Default::default(),
    )
    .map_err(|_| BytecodeVerificationError::InvalidEnumLayout)?;
    host::validate(
        &module.host_interface,
        &module.public_items,
        &module.structures,
        &module.enumerations,
        &Default::default(),
    )
    .map_err(|error| BytecodeVerificationError::InvalidHostInterface(format!("{error:?}")))?;
    if !host::trait_bindings_match(
        &module.host_interface,
        &module.identity,
        &module.public_items,
        &module.trait_contracts,
        &Default::default(),
    )
    .expect("bytecode verification uses an uncancelled token")
    {
        return Err(BytecodeVerificationError::InvalidHostInterface(
            "host trait table disagrees with script trait ABI".into(),
        ));
    }
    if module.function_table.len() != module.functions.len() {
        return Err(BytecodeVerificationError::FunctionTableLengthMismatch {
            functions: module.functions.len(),
            table: module.function_table.len(),
        });
    }
    let mut identities = HashSet::new();
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
            || record.identity != function.identity
            || record.name != function.name
            || record.params != function.metadata.params
            || record.return_type != function.metadata.return_type
            || record.effects != function.metadata.effects
        {
            return Err(BytecodeVerificationError::FunctionRecordMismatch {
                function: function.id,
            });
        }
        if let Some(identity) = &function.identity
            && (identity.declaration.module != module.identity
                || !identity.declaration.within_path_limit()
                || !identity.declaration.path.last().is_some_and(|part| {
                    matches!(part.kind, DefinitionKind::Function | DefinitionKind::Method)
                })
                || identity.arguments.iter().any(|ty| {
                    !ty.within_wire_limits() || !concrete_type_valid(ty, &Default::default())
                })
                || !identities.insert(identity))
        {
            return Err(BytecodeVerificationError::InvalidFunctionIdentity {
                function: function.id,
            });
        }
        verify_function(module, function, program)?;
    }
    verify_interface_tables(module)?;
    Ok(())
}

fn verify_interface_tables(module: &BytecodeModule) -> Result<(), BytecodeVerificationError> {
    let declared = module
        .public_items
        .iter()
        .filter_map(|item| match item {
            PublicItem::InterfaceTable(table) => Some(table),
            _ => None,
        })
        .collect::<Vec<_>>();
    if declared.iter().any(|abi| {
        module
            .interface_tables
            .iter()
            .filter(|table| table.declaration == abi.declaration && table.arguments.is_empty())
            .count()
            != 1
    }) {
        return Err(BytecodeVerificationError::InvalidInterfaceTable);
    }
    let mut instances = HashSet::new();
    for table in &module.interface_tables {
        if !instances.insert((&table.declaration, &table.arguments)) {
            return Err(BytecodeVerificationError::InvalidInterfaceTable);
        }
        let Some(abi) = declared
            .iter()
            .copied()
            .find(|abi| abi.declaration == table.declaration)
        else {
            return Err(BytecodeVerificationError::InvalidInterfaceTable);
        };
        if !table.arguments.is_empty()
            && abi
                .instantiate_in(&table.arguments, &abi.generic_params)
                .is_none()
        {
            return Err(BytecodeVerificationError::InvalidInterfaceTable);
        }
        if table.arguments.iter().any(|ty| !ty.is_concrete())
            && table.arguments
                != abi
                    .generic_params
                    .iter()
                    .map(|p| p.as_type())
                    .collect::<Vec<_>>()
        {
            return Err(BytecodeVerificationError::InvalidInterfaceTable);
        }
        let Ty::Trait(trait_type) = &abi.trait_type else {
            return Err(BytecodeVerificationError::InvalidInterfaceTable);
        };
        if table.declaration != abi.declaration {
            return Err(BytecodeVerificationError::InvalidInterfaceTable);
        }
        let mut used = HashSet::new();
        for slot in &table.methods {
            let Some(segment) = slot.method.path.last() else {
                return Err(BytecodeVerificationError::InvalidInterfaceTable);
            };
            if segment.kind != DefinitionKind::Method
                || segment.occurrence != 0
                || slot.method.module != trait_type.declaration.module
                || slot.method.path.len() != trait_type.declaration.path.len() + 1
                || slot.method.path[..slot.method.path.len() - 1] != trait_type.declaration.path
                || !used.insert(&slot.method)
            {
                return Err(BytecodeVerificationError::InvalidInterfaceTable);
            }
            let Some(method) = abi
                .methods
                .iter()
                .find(|method| method.name == segment.name)
            else {
                return Err(BytecodeVerificationError::InvalidInterfaceTable);
            };
            let identity = match slot.target {
                CallableTarget::Script(target) => {
                    let function = module
                        .functions
                        .get(target.index())
                        .ok_or(BytecodeVerificationError::InvalidInterfaceTable)?;
                    if matches!(
                        method.implementation,
                        CallableImplementation::Native(_)
                            | CallableImplementation::NativeDefault(_)
                    ) {
                        return Err(BytecodeVerificationError::InvalidInterfaceTable);
                    }
                    if abi.host_bridge && !host_bridge_method_matches(abi, slot, function, module) {
                        return Err(BytecodeVerificationError::InvalidInterfaceTable);
                    }
                    function
                        .identity
                        .as_ref()
                        .ok_or(BytecodeVerificationError::InvalidInterfaceTable)?
                }
                CallableTarget::Native(target) => {
                    let import = module
                        .native_imports
                        .get(target.index())
                        .ok_or(BytecodeVerificationError::InvalidInterfaceTable)?;
                    if abi.host_bridge
                        || import.host.is_some()
                        || match &method.implementation {
                            CallableImplementation::Native(binding) => binding != &import.binding,
                            CallableImplementation::NativeDefault(application) => {
                                application.declaration != import.instance.declaration
                                    || application.arguments.len()
                                        != import.instance.arguments.len()
                            }
                            _ => true,
                        }
                    {
                        return Err(BytecodeVerificationError::InvalidInterfaceTable);
                    }
                    &import.instance
                }
            };
            if matches!(
                method.implementation,
                CallableImplementation::NativeDefault(_)
            ) {
                // The linked proof checks the template, substituted arguments,
                // binding and signature against the complete dependency closure.
                continue;
            }
            if identity.declaration.module != abi.declaration.module
                || identity.declaration.path.len() != abi.declaration.path.len() + 1
                || identity.declaration.path[..identity.declaration.path.len() - 1]
                    != abi.declaration.path
                || identity.declaration.path.last() != Some(segment)
            {
                return Err(BytecodeVerificationError::InvalidInterfaceTable);
            }
            let body = match slot.target {
                CallableTarget::Script(target) => module.functions[target.index()]
                    .metadata
                    .semantic
                    .generic
                    .as_ref(),
                CallableTarget::Native(target) => {
                    module.native_imports[target.index()].generic.as_ref()
                }
            };
            if body.map_or(0, |body| body.parameters.len()) != slot.arguments.len()
                || identity.arguments.len()
                    + if matches!(slot.target, CallableTarget::Script(_)) {
                        slot.arguments.len()
                    } else {
                        0
                    }
                    != abi.generic_params.len() + method.generic_params.len()
            {
                return Err(BytecodeVerificationError::InvalidInterfaceTable);
            }
        }
        if (abi.generic_params.is_empty() || !table.arguments.is_empty())
            && abi
                .methods
                .iter()
                .filter(|method| {
                    method.generic_params.is_empty()
                        || method
                            .params
                            .first()
                            .is_some_and(|parameter| parameter.name == "self")
                })
                .any(|method| {
                    table
                        .methods
                        .iter()
                        .filter(|slot| {
                            slot.method
                                .path
                                .last()
                                .is_some_and(|part| part.name == method.name)
                        })
                        .count()
                        != 1
                })
        {
            return Err(BytecodeVerificationError::InvalidInterfaceTable);
        }
    }
    Ok(())
}

fn host_bridge_method_matches(
    table: &InterfaceTable,
    slot: &InterfaceMethodSlot,
    function: &BytecodeFunction,
    module: &BytecodeModule,
) -> bool {
    let Some((_, implementation)) = host::host_bridge_implementation(table, &module.host_interface)
    else {
        return false;
    };
    let Some(mapping) = implementation
        .methods
        .iter()
        .find(|mapping| mapping.trait_method == slot.method)
    else {
        return false;
    };
    let params = function.metadata.params.len();
    if function.instructions.len() != params + 2 {
        return false;
    }
    let BytecodeInstruction::Call {
        dst: Some(result),
        callee: CallTarget::Native(import),
        args,
    } = &function.instructions[params]
    else {
        return false;
    };
    if args.len() != params
        || !module
            .native_imports
            .get(import.index())
            .and_then(|import| import.host.as_ref())
            .is_some_and(|contract| contract.id == mapping.host_method)
    {
        return false;
    }
    function.instructions[..params]
        .iter()
        .zip(args)
        .enumerate()
        .all(|(index, (instruction, arg))| {
            matches!(instruction, BytecodeInstruction::LoadLocal { dst, local }
                if dst == arg && local.index() == index)
        })
        && matches!(&function.instructions[params + 1], BytecodeInstruction::Return(Some(value)) if value == result)
}

fn verify_function(
    module: &BytecodeModule,
    function: &BytecodeFunction,
    program: Option<&BytecodeProgram>,
) -> Result<(), BytecodeVerificationError> {
    if !matches!(
        function.instructions.last(),
        Some(
            BytecodeInstruction::Return(_)
                | BytecodeInstruction::Jump { .. }
                | BytecodeInstruction::Branch { .. }
                | BytecodeInstruction::Unreachable
        )
    ) {
        return Err(BytecodeVerificationError::InvalidOperation {
            function: function.id,
            reason: "function falls through without a terminator",
        });
    }
    verify_metadata_counts(function)?;
    verify_metadata_types(module, function)?;
    verify_root_layout(function)?;
    verify_debug_metadata(function)?;
    for target in &function.metadata.control_flow_targets {
        verify_jump(function, *target)?;
    }
    for instruction in &function.instructions {
        verify_instruction(module, function, instruction, program)?;
    }
    access::verify(module, function, program)?;
    Ok(())
}

fn verify_root_layout(function: &BytecodeFunction) -> Result<(), BytecodeVerificationError> {
    if function.metadata.roots
        != RootSlotLayout::from_types(&function.metadata.locals, &function.metadata.registers)
    {
        return Err(BytecodeVerificationError::InvalidRootLayout {
            function: function.id,
        });
    }
    Ok(())
}

fn verify_metadata_types(
    module: &BytecodeModule,
    function: &BytecodeFunction,
) -> Result<(), BytecodeVerificationError> {
    for ty in iter::once(&function.metadata.return_type)
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

fn contract_error(function: &BytecodeFunction, error: ContractError) -> BytecodeVerificationError {
    match error {
        ContractError::TypeMismatch {
            context,
            expected,
            found,
        } => BytecodeVerificationError::TypeMismatch {
            function: function.id,
            context,
            expected,
            found,
        },
        ContractError::Intrinsic { intrinsic, reason } => {
            BytecodeVerificationError::RuntimePrimitiveSignatureMismatch {
                function: function.id,
                intrinsic,
                reason,
            }
        }
        ContractError::InvalidOperation { reason } => BytecodeVerificationError::InvalidOperation {
            function: function.id,
            reason,
        },
    }
}

fn verify_standard_intrinsic_call(
    function: &BytecodeFunction,
    dst: Option<Register>,
    intrinsic: RuntimePrimitive,
    args: &[Register],
) -> Result<(), BytecodeVerificationError> {
    let args = args
        .iter()
        .map(|arg| register_ty(function, *arg))
        .collect::<Result<Vec<_>, _>>()?;
    let dst = dst.map(|dst| register_ty(function, dst)).transpose()?;
    contracts::verify_intrinsic(dst, intrinsic, &args)
        .map_err(|error| contract_error(function, error))
}

fn verify_call_dst(
    function: &BytecodeFunction,
    dst: Option<Register>,
    return_type: ValueType,
) -> Result<(), BytecodeVerificationError> {
    let dst = dst.map(|dst| register_ty(function, dst)).transpose()?;
    contracts::verify_call_dst(dst, return_type).map_err(|error| contract_error(function, error))
}

fn function_ref_exists(module: &BytecodeModule, target: FunctionRef) -> bool {
    target.index() < module.functions.len() && target.index() < module.function_table.len()
}

fn field_layout(
    module: &BytecodeModule,
    function: &BytecodeFunction,
    field: &FieldRef,
) -> Result<StructFieldLayout, BytecodeVerificationError> {
    module
        .structures
        .get(field.structure.index())
        .and_then(|layout| layout.apply(&field.arguments, &Default::default()))
        .and_then(|layout| layout.fields.get(field.slot as usize).cloned())
        .ok_or(BytecodeVerificationError::InvalidFieldReference {
            function: function.id,
            field: field.clone(),
        })
}

fn path_record<'a>(
    module: &'a BytecodeModule,
    function: &BytecodeFunction,
    path: PathId,
) -> Result<&'a PathRecord, BytecodeVerificationError> {
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
        ConstantOperand::I64(_) => ValueType::I64,
        ConstantOperand::F32(_) => ValueType::F32,
        ConstantOperand::F64(_) => ValueType::F64,
        ConstantOperand::U64(_) => ValueType::U64,
        ConstantOperand::Str(_) => ValueType::Str,
    }
}

fn ir_binary_op(op: BinaryOp) -> MirBinaryOp {
    match op {
        BinaryOp::Numeric(op) => MirBinaryOp::Numeric(op),
        BinaryOp::Add => MirBinaryOp::Add,
        BinaryOp::Sub => MirBinaryOp::Sub,
        BinaryOp::Mul => MirBinaryOp::Mul,
        BinaryOp::Div => MirBinaryOp::Div,
        BinaryOp::Rem => MirBinaryOp::Rem,
        BinaryOp::Eq => MirBinaryOp::Eq,
        BinaryOp::NotEq => MirBinaryOp::NotEq,
        BinaryOp::IdentityEq => MirBinaryOp::IdentityEq,
        BinaryOp::IdentityNotEq => MirBinaryOp::IdentityNotEq,
        BinaryOp::Lt => MirBinaryOp::Lt,
        BinaryOp::Gt => MirBinaryOp::Gt,
        BinaryOp::Le => MirBinaryOp::Le,
        BinaryOp::Ge => MirBinaryOp::Ge,
    }
}

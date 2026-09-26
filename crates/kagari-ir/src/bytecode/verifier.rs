use kagari_common::identity::DefinitionKind;
use std::{
    collections::HashSet,
    fmt::{self, Display, Formatter},
};

use crate::{
    bytecode::{
        BinaryOp, BytecodeFunction, BytecodeInstruction, BytecodeModule, CallTarget,
        ConstantOperand, FieldRef, FunctionRef, JumpTarget, LocalSlot, ModuleSlot, PathId,
        Register, StandardIntrinsic, StructId, UnaryOp,
    },
    module::ValueType,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BytecodeVerificationError {
    InvalidProgramGraph,
    InvalidStructLayout,
    InvalidEnumLayout,
    InvalidPublicAbi,
    InvalidInterfaceTable,
    InvalidPathLayout,
    InvalidStructId {
        function: FunctionRef,
        structure: StructId,
    },
    InvalidHostInterface(String),
    InvalidHostImport {
        function: FunctionRef,
        import: super::HostImportId,
    },
    InvalidOperation {
        function: FunctionRef,
        reason: &'static str,
    },
    FunctionTableLengthMismatch {
        functions: usize,
        table: usize,
    },
    FunctionRecordMismatch {
        function: FunctionRef,
    },
    InvalidFunctionIdentity {
        function: FunctionRef,
    },
    InvalidRootLayout {
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
    InvalidFieldReference {
        function: FunctionRef,
        field: FieldRef,
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
    StandardIntrinsicSignatureMismatch {
        function: FunctionRef,
        intrinsic: StandardIntrinsic,
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
            Self::StandardIntrinsicSignatureMismatch { .. } => {
                "KG_BYTECODE_STANDARD_INTRINSIC_SIGNATURE_MISMATCH"
            }
        }
    }
}

impl Display for BytecodeVerificationError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidProgramGraph => write!(f, "invalid executable module graph"),
            Self::InvalidStructLayout => write!(f, "invalid struct layouts"),
            Self::InvalidEnumLayout => write!(f, "invalid enum layouts"),
            Self::InvalidPublicAbi => write!(f, "invalid public ABI"),
            Self::InvalidInterfaceTable => write!(f, "invalid executable interface table"),
            Self::InvalidPathLayout => write!(f, "invalid host path layout"),
            Self::InvalidStructId {
                function,
                structure,
            } => write!(f, "invalid struct {structure:?} in {function:?}"),
            Self::InvalidHostInterface(reason) => write!(f, "invalid host interface: {reason}"),
            Self::InvalidHostImport { function, import } => {
                write!(f, "invalid host import {import:?} in {function:?}")
            }
            Self::InvalidOperation { function, reason } => {
                write!(f, "invalid operation in {function:?}: {reason}")
            }
            Self::FunctionTableLengthMismatch { functions, table } => write!(
                f,
                "function table length mismatch: {functions} functions, {table} table records"
            ),
            Self::FunctionRecordMismatch { function } => {
                write!(f, "function record mismatch for {function:?}")
            }
            Self::InvalidFunctionIdentity { function } => {
                write!(f, "invalid function identity for {function:?}")
            }
            Self::InvalidRootLayout { function } => {
                write!(f, "invalid GC root layout for {function:?}")
            }
            Self::MetadataCountMismatch {
                function,
                layout,
                expected,
                found,
            } => write!(
                f,
                "metadata count mismatch in {function:?} {layout}: expected {expected}, found {found}"
            ),
            Self::InvalidRegister { function, register } => {
                write!(f, "invalid register {register:?} in {function:?}")
            }
            Self::InvalidLocal { function, local } => {
                write!(f, "invalid local {local:?} in {function:?}")
            }
            Self::InvalidModuleSlot { function, slot } => {
                write!(f, "invalid module slot {slot:?} in {function:?}")
            }
            Self::InvalidFieldReference { function, field } => {
                write!(f, "invalid field id {field:?} in {function:?}")
            }
            Self::InvalidPathId { function, path } => {
                write!(f, "invalid path id {path:?} in {function:?}")
            }
            Self::ReadOnlyPath { function, path } => {
                write!(f, "write to read-only path {path:?} in {function:?}")
            }
            Self::InvalidFunctionRef { function, target } => write!(
                f,
                "invalid call target {target:?} referenced from {function:?}"
            ),
            Self::MissingConstant { function } => {
                write!(
                    f,
                    "instruction in {function:?} references a missing constant"
                )
            }
            Self::MissingType { function, ty } => {
                write!(f, "metadata in {function:?} references missing type {ty:?}")
            }
            Self::InvalidJumpTarget { function, target } => {
                write!(f, "invalid jump target {target:?} in {function:?}")
            }
            Self::TypeMismatch {
                function,
                context,
                expected,
                found,
            } => write!(
                f,
                "bytecode type mismatch in {function:?} {context}: expected {expected:?}, found {found:?}"
            ),
            Self::ArityMismatch {
                function,
                target,
                expected,
                found,
            } => write!(
                f,
                "call arity mismatch in {function:?} to {target:?}: expected {expected}, found {found}"
            ),
            Self::StandardIntrinsicSignatureMismatch {
                function,
                intrinsic,
                reason,
            } => write!(
                f,
                "standard intrinsic signature mismatch in {function:?} for {intrinsic:?}: {reason}"
            ),
        }
    }
}

impl std::error::Error for BytecodeVerificationError {}

pub fn verify_module(module: &BytecodeModule) -> Result<(), BytecodeVerificationError> {
    if !module.dependencies.is_empty() {
        return Err(BytecodeVerificationError::InvalidProgramGraph);
    }
    verify_module_with_program(module, None)?;
    if !super::trait_bounds::trait_bounds_match(module, &[module]) {
        return Err(BytecodeVerificationError::InvalidHostInterface(
            "trait output or host bound has no unique valid implementation".into(),
        ));
    }
    Ok(())
}

pub(super) fn verify_module_with_program(
    module: &BytecodeModule,
    program: Option<&super::BytecodeProgram>,
) -> Result<(), BytecodeVerificationError> {
    if module
        .paths
        .iter()
        .enumerate()
        .any(|(index, path)| path.id.index() != index || path.root_ty != ValueType::HostHandle)
    {
        return Err(BytecodeVerificationError::InvalidPathLayout);
    }
    crate::module::abi::verify::validate(
        &module.public_items,
        &module.identity,
        &Default::default(),
    )
    .map_err(|_| BytecodeVerificationError::InvalidPublicAbi)?;
    crate::module::abi::verify::validate_trait_contracts(
        &module.trait_contracts,
        &module.public_items,
        &module.identity,
        &Default::default(),
    )
    .map_err(|_| BytecodeVerificationError::InvalidPublicAbi)?;
    crate::module::layout::validate_layouts(&module.structures, &Default::default())
        .map_err(|_| BytecodeVerificationError::InvalidStructLayout)?;
    if !crate::module::layout::struct_abi_matches(
        &module.structures,
        &module.identity,
        &module.public_items,
        &Default::default(),
    )
    .expect("bytecode verification uses an uncancelled token")
    {
        return Err(BytecodeVerificationError::InvalidStructLayout);
    }
    if !crate::module::layout::enum_abi_matches(
        &module.enumerations,
        &module.identity,
        &module.public_items,
        &Default::default(),
    )
    .expect("bytecode verification uses an uncancelled token")
    {
        return Err(BytecodeVerificationError::InvalidEnumLayout);
    }
    crate::module::layout::validate_enum_layouts(
        &module.enumerations,
        &module.structures,
        &Default::default(),
    )
    .map_err(|_| BytecodeVerificationError::InvalidEnumLayout)?;
    crate::module::host::validate(
        &module.host_interface,
        &module.public_items,
        &module.structures,
        &module.enumerations,
        &Default::default(),
    )
    .map_err(|error| BytecodeVerificationError::InvalidHostInterface(format!("{error:?}")))?;
    if !crate::module::host::trait_bindings_match(
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
                    !ty.within_wire_limits()
                        || !crate::module::abi::verify::concrete_type_valid(ty, &Default::default())
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
    use crate::module::{PublicAbiItem, abi::AbiType};
    let declared = module
        .public_items
        .iter()
        .filter_map(|item| match item {
            PublicAbiItem::InterfaceTable(table) => Some(table),
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
            && (abi.instantiate(&table.arguments).is_none()
                || abi
                    .methods
                    .iter()
                    .any(|method| !method.generic_params.is_empty()))
        {
            return Err(BytecodeVerificationError::InvalidInterfaceTable);
        }
        let AbiType::Trait(trait_type) = &abi.trait_type else {
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
                || !used.insert(slot.function)
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
            let Some(function) = module.functions.get(slot.function.index()) else {
                return Err(BytecodeVerificationError::InvalidInterfaceTable);
            };
            if abi.host_bridge && !host_bridge_method_matches(abi, slot, function, module) {
                return Err(BytecodeVerificationError::InvalidInterfaceTable);
            }
            let Some(identity) = &function.identity else {
                return Err(BytecodeVerificationError::InvalidInterfaceTable);
            };
            if identity.declaration.module != abi.declaration.module
                || identity.declaration.path.len() != abi.declaration.path.len() + 1
                || identity.declaration.path[..identity.declaration.path.len() - 1]
                    != abi.declaration.path
                || identity.declaration.path.last() != Some(segment)
            {
                return Err(BytecodeVerificationError::InvalidInterfaceTable);
            }
            if identity.arguments.len() != abi.generic_params.len() + method.generic_params.len() {
                return Err(BytecodeVerificationError::InvalidInterfaceTable);
            }
            if !table.arguments.is_empty() && identity.arguments != table.arguments {
                return Err(BytecodeVerificationError::InvalidInterfaceTable);
            }
            let method_owner = &identity.declaration;
            let (impl_arguments, method_arguments) =
                identity.arguments.split_at(abi.generic_params.len());
            let expected_params = method
                .params
                .iter()
                .map(|parameter| {
                    instantiate_method_type(
                        &parameter.ty,
                        &abi.declaration,
                        impl_arguments,
                        method_owner,
                        method_arguments,
                    )
                    .map(|ty| ty.representation())
                })
                .collect::<Option<Vec<_>>>();
            let expected_return = instantiate_method_type(
                &method.return_type,
                &abi.declaration,
                impl_arguments,
                method_owner,
                method_arguments,
            )
            .map(|ty| ty.representation());
            if expected_params.as_ref() != Some(&function.metadata.params)
                || expected_return != Some(function.metadata.return_type)
            {
                return Err(BytecodeVerificationError::InvalidInterfaceTable);
            }
        }
        if (abi.generic_params.is_empty() || !table.arguments.is_empty())
            && abi
                .methods
                .iter()
                .filter(|method| method.generic_params.is_empty())
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
    table: &crate::module::InterfaceTableAbi,
    slot: &super::InterfaceMethodSlot,
    function: &BytecodeFunction,
    module: &BytecodeModule,
) -> bool {
    let Some((_, implementation)) =
        crate::module::host::host_bridge_implementation(table, &module.host_interface)
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
        callee: CallTarget::HostFunction(import),
        args,
    } = &function.instructions[params]
    else {
        return false;
    };
    if args.len() != params
        || !module
            .host_interface
            .functions
            .get(import.index())
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

fn instantiate_method_type(
    ty: &crate::module::abi::AbiType,
    impl_owner: &kagari_common::identity::DefinitionId,
    impl_arguments: &[crate::module::abi::AbiType],
    method_owner: &kagari_common::identity::DefinitionId,
    method_arguments: &[crate::module::abi::AbiType],
) -> Option<crate::module::abi::AbiType> {
    use crate::module::abi::{AbiType, NominalAbiType};
    let child = |ty: &AbiType| {
        instantiate_method_type(
            ty,
            impl_owner,
            impl_arguments,
            method_owner,
            method_arguments,
        )
    };
    let nominal = |ty: &NominalAbiType| {
        Some(NominalAbiType {
            associated_types: ty
                .associated_types
                .iter()
                .map(|(id, ty)| Some((id.clone(), child(ty)?)))
                .collect::<Option<_>>()?,
            declaration: ty.declaration.clone(),
            arguments: ty.arguments.iter().map(&child).collect::<Option<_>>()?,
        })
    };
    Some(match ty {
        AbiType::Parameter { owner, position } if owner == impl_owner => {
            impl_arguments.get(*position)?.clone()
        }
        AbiType::Parameter { owner, position } if owner == method_owner => {
            method_arguments.get(*position)?.clone()
        }
        AbiType::Projection { .. } | AbiType::Parameter { .. } | AbiType::SelfType(_) => {
            return None;
        }
        AbiType::Builtin(_) | AbiType::Host(_) => ty.clone(),
        AbiType::Tuple(types) => AbiType::Tuple(types.iter().map(child).collect::<Option<_>>()?),
        AbiType::Function { params, result } => AbiType::Function {
            params: params.iter().map(child).collect::<Option<_>>()?,
            result: Box::new(child(result)?),
        },
        AbiType::Array(element) => AbiType::Array(Box::new(child(element)?)),
        AbiType::Set(element) => AbiType::Set(Box::new(child(element)?)),
        AbiType::Map { key, value } => AbiType::Map {
            key: Box::new(child(key)?),
            value: Box::new(child(value)?),
        },
        AbiType::StandardEnum { kind, args } => AbiType::StandardEnum {
            kind: *kind,
            args: args.iter().map(child).collect::<Option<_>>()?,
        },
        AbiType::Struct(ty) => AbiType::Struct(nominal(ty)?),
        AbiType::Enum(ty) => AbiType::Enum(nominal(ty)?),
        AbiType::Trait(ty) => AbiType::Trait(nominal(ty)?),
    })
}

fn verify_function(
    module: &BytecodeModule,
    function: &BytecodeFunction,
    program: Option<&super::BytecodeProgram>,
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
    Ok(())
}

fn verify_root_layout(function: &BytecodeFunction) -> Result<(), BytecodeVerificationError> {
    if function.metadata.roots
        != super::RootSlotLayout::from_types(
            &function.metadata.locals,
            &function.metadata.registers,
        )
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
    program: Option<&super::BytecodeProgram>,
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
                UnaryOp::Neg => crate::module::UnaryOp::Neg,
                UnaryOp::Not => crate::module::UnaryOp::Not,
            };
            let ty = crate::module::contracts::unary_result(op, register_ty(function, *operand)?)
                .map_err(|error| contract_error(function, error))?;
            expect_register_ty(function, *dst, ty, "unary dst")?;
        }
        BytecodeInstruction::Binary { dst, op, lhs, rhs } => {
            let ty = crate::module::contracts::binary_result(
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
        BytecodeInstruction::MakeTuple { dst, elements }
        | BytecodeInstruction::MakeArray { dst, elements } => {
            expect_register_ty(function, *dst, ValueType::HeapObject, "aggregate dst")?;
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
            if captures.len() > callee.metadata.params.len() {
                return Err(BytecodeVerificationError::InvalidOperation {
                    function: function.id,
                    reason: "closure capture count exceeds parameter count",
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
        BytecodeInstruction::MakeInterface {
            dst,
            value,
            module: target,
            implementation,
        } => {
            use crate::module::PublicAbiItem;
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
                    PublicAbiItem::InterfaceTable(table)
                        if table.declaration == linked.declaration =>
                    {
                        table.instantiate(&linked.arguments)
                    }
                    _ => None,
                })
                .ok_or(BytecodeVerificationError::InvalidInterfaceTable)?;
            if !table.generic_params.is_empty()
                || !table.for_type.is_concrete()
                || !table.trait_type.is_concrete()
                || table
                    .methods
                    .iter()
                    .any(|method| !method.generic_params.is_empty())
            {
                return Err(BytecodeVerificationError::InvalidInterfaceTable);
            }
            expect_register_ty(
                function,
                *value,
                table.for_type.representation(),
                "interface receiver",
            )?;
        }
        BytecodeInstruction::MakeEnum {
            dst,
            enumeration,
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
                .and_then(|layout| layout.variants.get(*variant as usize))
                .ok_or_else(invalid)?;
            if fields.len() != layout.payload.len() {
                return Err(invalid());
            }
            for (register, ty) in fields.iter().zip(&layout.payload) {
                expect_register_ty(function, *register, ty.representation(), "enum payload")?;
            }
        }
        BytecodeInstruction::TestEnumVariant {
            dst,
            value,
            enumeration,
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
                .and_then(|layout| layout.variants.get(*variant as usize))
                .ok_or(BytecodeVerificationError::InvalidOperation {
                    function: function.id,
                    reason: "enum pattern variant",
                })?;
        }
        BytecodeInstruction::ReadEnumPayload {
            dst,
            value,
            enumeration,
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
                .and_then(|layout| layout.variants.get(*variant as usize))
                .and_then(|variant| variant.payload.get(*index as usize))
                .ok_or(BytecodeVerificationError::InvalidOperation {
                    function: function.id,
                    reason: "enum pattern payload",
                })?;
            expect_register_ty(
                function,
                *dst,
                ty.representation(),
                "enum pattern payload dst",
            )?;
        }
        BytecodeInstruction::MakeStruct {
            dst,
            structure,
            fields,
        } => {
            expect_register_ty(function, *dst, ValueType::HeapObject, "struct dst")?;
            let layout = module.structures.get(structure.index()).ok_or(
                BytecodeVerificationError::InvalidStructId {
                    function: function.id,
                    structure: *structure,
                },
            )?;
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
                    field.ty.representation(),
                    "struct field initializer",
                )?;
            }
        }
        BytecodeInstruction::ReadAggregateField { dst, base, field } => {
            let field = field_layout(module, function, *field)?;
            expect_register_ty(
                function,
                *dst,
                field.ty.representation(),
                "aggregate field dst",
            )?;
            expect_register_ty(function, *base, ValueType::HeapObject, "field base")?;
        }
        BytecodeInstruction::WriteAggregateField { base, field, value } => {
            let field = field_layout(module, function, *field)?;
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
                field.ty.representation(),
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
            let result = crate::module::contracts::binary_result(
                ir_binary_op(*op),
                path.result_ty,
                path.result_ty,
            )
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
    program: Option<&super::BytecodeProgram>,
) -> Result<(), BytecodeVerificationError> {
    match callee {
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
            interface,
            method_slot,
        } => {
            let owner = if let Some(program) = program {
                program.modules.get(owner_slot.index())
            } else if owner_slot.index() == 0 {
                Some(module)
            } else {
                None
            }
            .ok_or(BytecodeVerificationError::InvalidProgramGraph)?;
            let (params, return_type) = crate::module::abi::interface_method_types(
                &owner.identity,
                &owner.public_items,
                &owner.trait_contracts,
                interface,
                *method_slot as usize,
            )
            .ok_or(BytecodeVerificationError::InvalidOperation {
                function: function.id,
                reason: "invalid linked interface method",
            })?;
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
        CallTarget::HostFunction(import) => {
            let declaration = module.host_interface.functions.get(import.index()).ok_or(
                BytecodeVerificationError::InvalidHostImport {
                    function: function.id,
                    import: *import,
                },
            )?;
            let args = args
                .iter()
                .map(|arg| register_ty(function, *arg))
                .collect::<Result<Vec<_>, _>>()?;
            let dst = dst.map(|dst| register_ty(function, dst)).transpose()?;
            crate::module::contracts::verify_host_call(dst, declaration, &args)
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
        CallTarget::StandardIntrinsic(intrinsic) => {
            verify_standard_intrinsic_call(function, dst, *intrinsic, args)?;
        }
        CallTarget::RuntimeHelper(super::RuntimeHelper::DynamicCall) => {
            return Err(BytecodeVerificationError::InvalidOperation {
                function: function.id,
                reason: "dynamic invocation has no executable contract",
            });
        }
        CallTarget::RuntimeHelper(helper) => {
            let kind = match helper {
                super::RuntimeHelper::ReflectTypeOf => {
                    crate::module::contracts::RuntimeHelperKind::TypeOf
                }
                super::RuntimeHelper::ReflectGetField(_) => {
                    crate::module::contracts::RuntimeHelperKind::GetField
                }
                super::RuntimeHelper::ReflectSetField(_) => {
                    crate::module::contracts::RuntimeHelperKind::SetField
                }
                super::RuntimeHelper::ReflectSetIndex => {
                    crate::module::contracts::RuntimeHelperKind::SetIndex
                }
                super::RuntimeHelper::DynamicCall => unreachable!(),
            };
            let args = args
                .iter()
                .map(|arg| register_ty(function, *arg))
                .collect::<Result<Vec<_>, _>>()?;
            let dst = dst.map(|dst| register_ty(function, dst)).transpose()?;
            crate::module::contracts::verify_runtime_helper_call(dst, kind, &args)
                .map_err(|error| contract_error(function, error))?;
        }
    }
    Ok(())
}

fn contract_error(
    function: &BytecodeFunction,
    error: crate::module::contracts::ContractError,
) -> BytecodeVerificationError {
    use crate::module::contracts::ContractError;
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
            BytecodeVerificationError::StandardIntrinsicSignatureMismatch {
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
    intrinsic: StandardIntrinsic,
    args: &[Register],
) -> Result<(), BytecodeVerificationError> {
    let args = args
        .iter()
        .map(|arg| register_ty(function, *arg))
        .collect::<Result<Vec<_>, _>>()?;
    let dst = dst.map(|dst| register_ty(function, dst)).transpose()?;
    crate::module::contracts::verify_intrinsic(dst, intrinsic, &args)
        .map_err(|error| contract_error(function, error))
}

fn verify_call_dst(
    function: &BytecodeFunction,
    dst: Option<Register>,
    return_type: ValueType,
) -> Result<(), BytecodeVerificationError> {
    let dst = dst.map(|dst| register_ty(function, dst)).transpose()?;
    crate::module::contracts::verify_call_dst(dst, return_type)
        .map_err(|error| contract_error(function, error))
}
fn function_ref_exists(module: &BytecodeModule, target: FunctionRef) -> bool {
    target.index() < module.functions.len() && target.index() < module.function_table.len()
}

fn field_layout<'a>(
    module: &'a BytecodeModule,
    function: &BytecodeFunction,
    field: FieldRef,
) -> Result<&'a crate::module::StructFieldLayout, BytecodeVerificationError> {
    module
        .structures
        .get(field.structure.index())
        .and_then(|layout| layout.fields.get(field.slot as usize))
        .ok_or(BytecodeVerificationError::InvalidFieldReference {
            function: function.id,
            field,
        })
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
        ConstantOperand::I64(_) => ValueType::I64,
        ConstantOperand::F32(_) => ValueType::F32,
        ConstantOperand::Str(_) => ValueType::Str,
    }
}

fn ir_binary_op(op: BinaryOp) -> crate::module::BinaryOp {
    use crate::module::BinaryOp as Ir;
    match op {
        BinaryOp::Add => Ir::Add,
        BinaryOp::Sub => Ir::Sub,
        BinaryOp::Mul => Ir::Mul,
        BinaryOp::Div => Ir::Div,
        BinaryOp::Rem => Ir::Rem,
        BinaryOp::Eq => Ir::Eq,
        BinaryOp::NotEq => Ir::NotEq,
        BinaryOp::Lt => Ir::Lt,
        BinaryOp::Gt => Ir::Gt,
        BinaryOp::Le => Ir::Le,
        BinaryOp::Ge => Ir::Ge,
    }
}

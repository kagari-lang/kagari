use std::collections::HashMap;

use kagari_common::Span;

use crate::bytecode::instruction::{
    BinaryOp, BytecodeInstruction, CallTarget, ConstantOperand, FieldRef, FunctionRef,
    HostImportId, InterfaceTableRef, JumpTarget, LocalSlot, ModuleSlot, PathId, Register,
    RuntimeHelper, StructId, UnaryOp,
};
use crate::bytecode::module::{
    BytecodeDebugMetadata, BytecodeFunction, BytecodeModule, BytecodeModuleSlot,
    CapturedBindingDebugInfo, DebugPointId, FrameLayout, FunctionMetadata, FunctionRecord,
    InstructionSourceSpan, InterfaceMethodSlot, InterfaceTableRecord, LineTableEntry,
    LocalLiveRange, PathRecord, RootSlotLayout, SafeDebugPoint, SafeDebugPointKind,
};
use crate::bytecode::verify_module;
use crate::module::{
    ValueType, VerifiedIrModule,
    function::{BasicBlock, IrFunction},
    ids::{BlockId, LocalId, ModuleSlotId, TempId},
    instruction::{
        AggregateFieldRef, BinaryOp as IrBinaryOp, CallTarget as IrCallTarget, Constant,
        Instruction, IrValue, PathRef, RuntimeHelper as IrRuntimeHelper, Terminator,
        UnaryOp as IrUnaryOp,
    },
};

#[derive(Debug)]
pub enum BytecodeLoweringError {
    UnlinkedSourceModules,
    InvalidBranchTarget(BlockId),
    Verification(crate::bytecode::BytecodeVerificationError),
}

pub fn lower_to_bytecode(ir: &VerifiedIrModule) -> Result<BytecodeModule, BytecodeLoweringError> {
    if !ir.dependencies.is_empty()
        || ir
            .functions
            .iter()
            .flat_map(|f| &f.blocks)
            .flat_map(|b| &b.instructions)
            .any(|i| match i {
                Instruction::Call {
                    callee: IrCallTarget::SourceFunction(_),
                    ..
                } => true,
                Instruction::Call {
                    callee: IrCallTarget::InterfaceMethod(contract),
                    ..
                } => contract.interface.declaration.module != ir.identity,
                Instruction::MakeInterface { implementation, .. } => {
                    implementation.module != ir.identity
                }
                _ => false,
            })
    {
        return Err(BytecodeLoweringError::UnlinkedSourceModules);
    }
    let module = lower_linked_module(ir, None, Vec::new())?;
    verify_module(&module).map_err(BytecodeLoweringError::Verification)?;
    Ok(module)
}

fn lower_linked_module(
    ir: &VerifiedIrModule,
    program: Option<&crate::program::VerifiedIrProgram>,
    dependencies: Vec<super::ModuleRef>,
) -> Result<BytecodeModule, BytecodeLoweringError> {
    let mut context = BytecodeLoweringContext {
        program,
        structures: &ir.structures,
        enumerations: &ir.enumerations,
        identity: Some(&ir.identity),
        ir: Some(ir),
        host_interface: kagari_common::host_interface::HostInterface {
            paths: vec![],
            types: ir.host_types.clone(),
            functions: Vec::new(),
        },
        ..Default::default()
    };
    let functions = ir
        .functions
        .iter()
        .map(|function| lower_function(function, &mut context))
        .collect::<Result<Vec<_>, _>>()?;
    let mut module = BytecodeModule {
        dependencies,
        host_interface: context.host_interface,
        identity: ir.identity.clone(),
        source_name: ir.source_name.clone(),
        module_slots: ir
            .module_slots
            .iter()
            .map(|slot| BytecodeModuleSlot {
                name: slot.name.clone(),
                ty: slot.ty,
                mutable: slot.mutable,
            })
            .collect(),
        constants: Vec::new(),
        types: Vec::new(),
        structures: ir.structures.clone(),
        enumerations: ir.enumerations.clone(),
        interface_tables: context
            .interface_tables
            .remove(&ir.identity)
            .unwrap_or_else(|| collect_interface_tables(ir, program)),
        paths: context.paths,
        function_table: Vec::new(),
        public_items: ir.abi.public_items.clone(),
        trait_contracts: ir.abi.trait_contracts.clone(),
        functions,
    };
    module.constants = collect_constant_pool(&module.functions);
    module.types = collect_type_table(&module);
    module.function_table = collect_function_table(&module.functions);

    Ok(module)
}

fn collect_interface_tables(
    ir: &VerifiedIrModule,
    program: Option<&crate::program::VerifiedIrProgram>,
) -> Vec<InterfaceTableRecord> {
    use crate::module::{PublicAbiItem, abi::AbiType};
    use kagari_common::identity::{DefinitionId, DefinitionKind, DefinitionPathSegment};
    let mut tables: Vec<_> = ir
        .abi
        .public_items
        .iter()
        .filter_map(|item| {
            let PublicAbiItem::InterfaceTable(table) = item else {
                return None;
            };
            let AbiType::Trait(trait_type) = &table.trait_type else {
                unreachable!("verified interface trait type")
            };
            let mut methods = Vec::new();
            for declared in &table.methods {
                let segment = DefinitionPathSegment {
                    kind: DefinitionKind::Method,
                    name: declared.name.clone(),
                    occurrence: 0,
                };
                let mut impl_path = table.declaration.path.clone();
                impl_path.push(segment.clone());
                let implementation = DefinitionId {
                    module: ir.identity.clone(),
                    path: impl_path,
                };
                let mut trait_path = trait_type.declaration.path.clone();
                trait_path.push(segment);
                let method = DefinitionId {
                    module: trait_type.declaration.module.clone(),
                    path: trait_path,
                };
                for function in &ir.functions {
                    if function.instance.declaration == implementation {
                        methods.push(InterfaceMethodSlot {
                            method: method.clone(),
                            function: FunctionRef::new(function.id.index()),
                        });
                    }
                }
            }
            Some(InterfaceTableRecord {
                arguments: Vec::new(),
                declaration: table.declaration.clone(),
                methods,
            })
        })
        .collect();
    let owners = program
        .map(|program| program.modules())
        .unwrap_or(std::slice::from_ref(ir));
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
            } => Some(crate::module::function::FunctionInstance {
                declaration: implementation.clone(),
                arguments: arguments
                    .iter()
                    .map(crate::module::abi::AbiType::to_checked_type)
                    .collect(),
            }),
            _ => None,
        });
    let demands = owners
        .iter()
        .flat_map(|owner| owner.interface_instances.iter().cloned());
    for instance in allocations.chain(demands) {
        let implementation = &instance.declaration;
        let arguments = instance
            .arguments
            .iter()
            .map(crate::module::abi::AbiType::from_checked_type)
            .collect::<Vec<_>>();
        if implementation.module != ir.identity
            || arguments.is_empty()
            || tables
                .iter()
                .any(|table| table.declaration == *implementation && table.arguments == arguments)
        {
            continue;
        }
        let base = tables
            .iter()
            .find(|table| table.declaration == *implementation && table.arguments.is_empty())
            .expect("verified impl template");
        let methods = base
            .methods
            .iter()
            .filter(|method| {
                ir.functions[method.function.index()]
                    .instance
                    .arguments
                    .iter()
                    .map(crate::module::abi::AbiType::from_checked_type)
                    .eq(arguments.iter().cloned())
            })
            .cloned()
            .collect();
        tables.push(InterfaceTableRecord {
            declaration: implementation.clone(),
            arguments: arguments.clone(),
            methods,
        });
    }
    tables
}

#[derive(Debug, Default)]
struct BytecodeLoweringContext<'a> {
    program: Option<&'a crate::program::VerifiedIrProgram>,
    structures: &'a [crate::module::StructLayout],
    enumerations: &'a [crate::module::EnumLayout],
    identity: Option<&'a kagari_common::identity::ModuleIdentity>,
    ir: Option<&'a VerifiedIrModule>,
    interface_tables: HashMap<kagari_common::identity::ModuleIdentity, Vec<InterfaceTableRecord>>,
    host_interface: kagari_common::host_interface::HostInterface,
    paths: Vec<PathRecord>,
}

impl BytecodeLoweringContext<'_> {
    fn owner_ref(&self, owner: &kagari_common::identity::ModuleIdentity) -> super::ModuleRef {
        if let Some(program) = self.program {
            let index = program
                .modules()
                .iter()
                .position(|module| &module.identity == owner)
                .expect("verified interface owner module");
            super::ModuleRef::new(index)
        } else {
            assert_eq!(self.identity.expect("lowering module identity"), owner);
            super::ModuleRef::new(0)
        }
    }

    fn interface_ref(
        &mut self,
        implementation: &kagari_common::identity::DefinitionId,
        arguments: &[crate::module::abi::AbiType],
    ) -> (super::ModuleRef, InterfaceTableRef) {
        let (module, owner) = if let Some(program) = self.program {
            program
                .modules()
                .iter()
                .enumerate()
                .find(|(_, owner)| owner.identity == implementation.module)
                .map(|(index, owner)| (super::ModuleRef::new(index), owner))
                .expect("verified interface owner")
        } else {
            (super::ModuleRef::new(0), self.ir.expect("lowering module"))
        };
        let tables = self
            .interface_tables
            .entry(owner.identity.clone())
            .or_insert_with(|| collect_interface_tables(owner, self.program));
        let table = tables
            .iter()
            .position(|table| table.declaration == *implementation && table.arguments == arguments)
            .expect("verified interface instance");
        (module, InterfaceTableRef::new(table))
    }

    fn host_import(
        &mut self,
        declaration: &kagari_common::host_interface::HostFunctionDeclaration,
    ) -> HostImportId {
        if let Some(index) = self
            .host_interface
            .functions
            .iter()
            .position(|entry| entry.id == declaration.id)
        {
            return HostImportId::new(index);
        }
        let id = HostImportId::new(self.host_interface.functions.len());
        self.host_interface.functions.push(declaration.clone());
        id
    }
    fn structure_id(&self, id: &crate::module::abi::NominalAbiType) -> StructId {
        StructId::new(
            self.structures
                .iter()
                .position(|layout| {
                    layout.declaration == id.declaration && layout.arguments == id.arguments
                })
                .expect("verified struct layout"),
        )
    }
    fn field_ref(&self, field: &AggregateFieldRef) -> FieldRef {
        FieldRef {
            structure: self.structure_id(&field.owner),
            slot: field.slot as u32,
        }
    }

    fn path_id(&mut self, path: &PathRef) -> PathId {
        if let Some(declaration) = &path.declaration
            && !self.host_interface.paths.contains(declaration)
        {
            self.host_interface.paths.push(declaration.clone());
        }
        if let Some(record) = self.paths.iter().find(|record| {
            record.contract_fingerprint == path.contract_fingerprint
                && record.root_ty == path.root_ty
                && record.result_ty == path.result_ty
                && record.read_only == path.read_only
                && record.debug_name == path.debug_name
        }) {
            return record.id;
        }
        let id = PathId::new(self.paths.len());
        self.paths.push(PathRecord {
            contract_fingerprint: path.contract_fingerprint,
            id,
            root_ty: path.root_ty,
            result_ty: path.result_ty,
            read_only: path.read_only,
            debug_name: path.debug_name.clone(),
        });
        id
    }
}

fn lower_function(
    function: &IrFunction,
    context: &mut BytecodeLoweringContext,
) -> Result<BytecodeFunction, BytecodeLoweringError> {
    let block_offsets = compute_block_offsets(function);
    let mut instructions = Vec::with_capacity(
        function
            .blocks
            .iter()
            .map(|block| block.instructions.len() + usize::from(block.terminator.is_some()))
            .sum(),
    );
    let mut instruction_spans = Vec::with_capacity(instructions.capacity());
    let mut instruction_scopes = Vec::with_capacity(instructions.capacity());

    for (_, block) in emission_order(function) {
        instruction_scopes.extend(&block.instruction_scopes);
        if block.terminator.is_some() {
            instruction_scopes.push(block.terminator_scope.unwrap_or(0));
        }
        lower_block(
            block,
            &block_offsets,
            context,
            &mut instructions,
            &mut instruction_spans,
        )?;
    }

    let (root_locals, root_temps) = function.root_slots();
    let metadata = FunctionMetadata {
        semantic: function.semantic.clone(),
        params: function.params.iter().map(|param| param.ty).collect(),
        return_type: function.return_type,
        locals: function.locals.iter().map(|local| local.ty).collect(),
        registers: function.temps.iter().map(|temp| temp.ty).collect(),
        roots: RootSlotLayout {
            locals: root_locals
                .into_iter()
                .map(|local| LocalSlot::new(local.index()))
                .collect(),
            registers: root_temps
                .into_iter()
                .map(|temp| Register::new(temp.index()))
                .collect(),
        },
        control_flow_targets: collect_control_flow_targets(&instructions),
        effects: function.effects,
        debug: collect_debug_metadata(
            function,
            &instructions,
            &instruction_spans,
            &instruction_scopes,
            function
                .debug
                .source_module
                .as_ref()
                .map(|owner| context.owner_ref(owner)),
        ),
    };

    Ok(BytecodeFunction {
        id: FunctionRef::new(function.id.index()),
        identity: Some(crate::module::ConcreteFunctionIdentity::from_ir(
            &function.instance,
        )),
        name: function.name.clone(),
        parameter_count: function.params.len() as u16,
        register_count: function.temps.len() as u16,
        local_count: function.locals.len() as u16,
        metadata,
        instructions,
    })
}

fn collect_function_table(functions: &[BytecodeFunction]) -> Vec<FunctionRecord> {
    functions
        .iter()
        .map(|function| FunctionRecord {
            id: function.id,
            identity: function.identity.clone(),
            name: function.name.clone(),
            params: function.metadata.params.clone(),
            return_type: function.metadata.return_type,
            effects: function.metadata.effects,
        })
        .collect()
}

fn collect_constant_pool(functions: &[BytecodeFunction]) -> Vec<ConstantOperand> {
    let mut constants = Vec::new();
    for function in functions {
        for instruction in &function.instructions {
            if let BytecodeInstruction::LoadConst { constant, .. } = instruction
                && !constants.contains(constant)
            {
                constants.push(constant.clone());
            }
        }
    }
    constants
}

fn collect_type_table(module: &BytecodeModule) -> Vec<ValueType> {
    let mut types = Vec::new();
    for slot in &module.module_slots {
        push_type(&mut types, slot.ty);
    }
    for layout in &module.structures {
        for field in &layout.fields {
            push_type(&mut types, field.ty.representation());
        }
    }
    for layout in &module.enumerations {
        for variant in &layout.variants {
            for ty in &variant.payload {
                push_type(&mut types, ty.representation());
            }
        }
    }
    for path in &module.paths {
        push_type(&mut types, path.root_ty);
        push_type(&mut types, path.result_ty);
    }
    for function in &module.functions {
        push_type(&mut types, function.metadata.return_type);
        for ty in function
            .metadata
            .params
            .iter()
            .chain(&function.metadata.locals)
            .chain(&function.metadata.registers)
        {
            push_type(&mut types, *ty);
        }
    }
    types
}

fn push_type(types: &mut Vec<ValueType>, ty: ValueType) {
    if !types.contains(&ty) {
        types.push(ty);
    }
}

fn collect_control_flow_targets(instructions: &[BytecodeInstruction]) -> Vec<JumpTarget> {
    let mut targets = Vec::new();
    for instruction in instructions {
        match instruction {
            BytecodeInstruction::Jump { target } => push_target(&mut targets, *target),
            BytecodeInstruction::Branch {
                then_target,
                else_target,
                ..
            } => {
                push_target(&mut targets, *then_target);
                push_target(&mut targets, *else_target);
            }
            _ => {}
        }
    }
    targets
}

fn collect_debug_metadata(
    function: &IrFunction,
    instructions: &[BytecodeInstruction],
    instruction_spans: &[Span],
    instruction_scopes: &[usize],
    source_module: Option<super::ModuleRef>,
) -> BytecodeDebugMetadata {
    let source_spans = instruction_spans
        .iter()
        .enumerate()
        .map(|(instruction_offset, span)| InstructionSourceSpan {
            instruction_offset,
            span: *span,
        })
        .collect::<Vec<_>>();

    let line_table = instruction_spans
        .iter()
        .enumerate()
        .map(|(instruction_offset, span)| {
            let position = function.debug.source.as_ref().and_then(|source| {
                source.position(
                    span.start,
                    kagari_common::line_index::PositionEncoding::Utf8,
                )
            });
            LineTableEntry {
                instruction_offset,
                source_offset: span.start,
                line: position.and_then(|p| u32::try_from(p.line + 1).ok()),
                column: position.and_then(|p| u32::try_from(p.character + 1).ok()),
            }
        })
        .collect::<Vec<_>>();

    let mut safe_debug_points = Vec::new();
    if !instructions.is_empty() {
        push_debug_point(
            &mut safe_debug_points,
            0,
            instruction_spans.first().copied().unwrap_or_default(),
            SafeDebugPointKind::FunctionEntry,
        );
    }
    for (instruction_offset, instruction) in instructions.iter().enumerate() {
        let span = instruction_spans
            .get(instruction_offset)
            .copied()
            .unwrap_or_default();
        match instruction {
            BytecodeInstruction::Call { .. } => push_debug_point(
                &mut safe_debug_points,
                instruction_offset,
                span,
                SafeDebugPointKind::CallBoundary,
            ),
            BytecodeInstruction::Jump { .. } | BytecodeInstruction::Branch { .. } => {
                push_debug_point(
                    &mut safe_debug_points,
                    instruction_offset,
                    span,
                    SafeDebugPointKind::BranchTarget,
                );
            }
            BytecodeInstruction::Return(_) => push_debug_point(
                &mut safe_debug_points,
                instruction_offset,
                span,
                SafeDebugPointKind::FunctionReturn,
            ),
            BytecodeInstruction::Unreachable => push_debug_point(
                &mut safe_debug_points,
                instruction_offset,
                span,
                SafeDebugPointKind::Trap,
            ),
            _ if span != Span::default() => push_debug_point(
                &mut safe_debug_points,
                instruction_offset,
                span,
                SafeDebugPointKind::Statement,
            ),
            _ => {}
        }
    }

    let local_live_ranges = collect_local_live_ranges(function, instructions, instruction_scopes);
    let captured_bindings = function
        .debug
        .captured_bindings
        .iter()
        .map(|captured| CapturedBindingDebugInfo {
            name: captured.name.clone(),
            span: captured.span,
            ty: captured.ty,
        })
        .collect();

    BytecodeDebugMetadata {
        source_uri: function
            .debug
            .source
            .as_ref()
            .map(|source| source.name().to_owned()),
        source_module,
        function_span: function.debug.source_span,
        source_spans,
        line_table,
        safe_debug_points,
        local_live_ranges,
        captured_bindings,
        frame_layout: FrameLayout {
            params: function.params.iter().map(|param| param.ty).collect(),
            locals: function.locals.iter().map(|local| local.ty).collect(),
            registers: function.temps.iter().map(|temp| temp.ty).collect(),
        },
    }
}

fn collect_local_live_ranges(
    function: &IrFunction,
    instructions: &[BytecodeInstruction],
    instruction_scopes: &[usize],
) -> Vec<LocalLiveRange> {
    let end = instructions.len();
    let mut ranges = Vec::new();
    let locals = function
        .debug
        .locals
        .iter()
        .map(|local| (local.local, local))
        .collect::<HashMap<_, _>>();
    for local in function
        .debug
        .locals
        .iter()
        .filter(|local| local.is_parameter)
    {
        ranges.push(LocalLiveRange {
            local: lower_local(local.local),
            name: local.name.clone(),
            span: local.span,
            start: 0,
            end,
            ty: local.ty,
            is_parameter: true,
        });
    }

    let scopes = &function.debug.lexical_scopes;
    let mut previous_path = Vec::<usize>::new();
    let mut open = HashMap::<LocalId, usize>::new();
    for offset in 0..=end {
        let mut next_path = Vec::<usize>::new();
        if offset < end && !scopes.is_empty() {
            let mut scope = instruction_scopes.get(offset).copied().unwrap_or(0);
            while let Some(entry) = scopes.get(scope) {
                next_path.push(scope);
                if next_path.len() >= scopes.len() {
                    break;
                }
                let Some(parent) = entry.parent else { break };
                scope = parent;
            }
            next_path.reverse();
        }
        let common = previous_path
            .iter()
            .zip(&next_path)
            .take_while(|(left, right)| left == right)
            .count();
        for scope in previous_path[common..].iter().rev() {
            if let Some(local) = scopes[*scope].local
                && let Some(start) = open.remove(&local)
                && start < offset
                && let Some(info) = locals.get(&local)
            {
                ranges.push(LocalLiveRange {
                    local: lower_local(local),
                    name: info.name.clone(),
                    span: info.span,
                    start,
                    end: offset,
                    ty: info.ty,
                    is_parameter: false,
                });
            }
        }
        for scope in &next_path[common..] {
            if let Some(local) = scopes[*scope].local {
                open.insert(local, offset);
            }
        }
        previous_path = next_path;
    }
    ranges.sort_by_key(|range| (range.local.index(), range.start));
    ranges
}

fn push_debug_point(
    points: &mut Vec<SafeDebugPoint>,
    instruction_offset: usize,
    span: Span,
    kind: SafeDebugPointKind,
) {
    if points
        .iter()
        .any(|point| point.instruction_offset == instruction_offset && point.kind == kind)
    {
        return;
    }
    let id = DebugPointId::new(points.len());
    points.push(SafeDebugPoint {
        id,
        instruction_offset,
        span,
        kind,
    });
}

fn push_target(targets: &mut Vec<JumpTarget>, target: JumpTarget) {
    if !targets.contains(&target) {
        targets.push(target);
    }
}

fn compute_block_offsets(function: &IrFunction) -> HashMap<BlockId, JumpTarget> {
    let mut offsets = HashMap::new();
    let mut next_offset = 0usize;

    for (index, block) in emission_order(function) {
        let block_id = BlockId::new(index);
        offsets.insert(block_id, JumpTarget::new(next_offset));
        next_offset += block.instructions.len();
        if block.terminator.is_some() {
            next_offset += 1;
        }
    }

    offsets
}

fn lower_block(
    block: &BasicBlock,
    block_offsets: &HashMap<BlockId, JumpTarget>,
    context: &mut BytecodeLoweringContext,
    out: &mut Vec<BytecodeInstruction>,
    spans: &mut Vec<Span>,
) -> Result<(), BytecodeLoweringError> {
    for (index, instruction) in block.instructions.iter().enumerate() {
        out.push(lower_instruction(instruction, context));
        spans.push(
            block
                .instruction_spans
                .get(index)
                .copied()
                .unwrap_or_default(),
        );
    }

    if let Some(terminator) = &block.terminator {
        out.push(lower_terminator(terminator, block_offsets)?);
        spans.push(block.terminator_span.unwrap_or_default());
    }

    Ok(())
}

fn lower_instruction(
    instruction: &Instruction,
    context: &mut BytecodeLoweringContext,
) -> BytecodeInstruction {
    match instruction {
        Instruction::LoadConst { dst, constant } => BytecodeInstruction::LoadConst {
            dst: lower_value(*dst),
            constant: lower_constant(constant),
        },
        Instruction::LoadLocal { dst, local } => BytecodeInstruction::LoadLocal {
            dst: lower_value(*dst),
            local: lower_local(*local),
        },
        Instruction::LoadModule { dst, slot } => BytecodeInstruction::LoadModule {
            dst: lower_value(*dst),
            slot: lower_module_slot(*slot),
        },
        Instruction::StoreLocal { local, src } => BytecodeInstruction::StoreLocal {
            local: lower_local(*local),
            src: lower_value(*src),
        },
        Instruction::StoreModule { slot, src } => BytecodeInstruction::StoreModule {
            slot: lower_module_slot(*slot),
            src: lower_value(*src),
        },
        Instruction::Move { dst, src } => BytecodeInstruction::Move {
            dst: lower_value(*dst),
            src: lower_value(*src),
        },
        Instruction::Unary { dst, op, operand } => BytecodeInstruction::Unary {
            dst: lower_value(*dst),
            op: match op {
                IrUnaryOp::Neg => UnaryOp::Neg,
                IrUnaryOp::Not => UnaryOp::Not,
            },
            operand: lower_value(*operand),
        },
        Instruction::Binary { dst, op, lhs, rhs } => BytecodeInstruction::Binary {
            dst: lower_value(*dst),
            op: lower_binary_op(*op),
            lhs: lower_value(*lhs),
            rhs: lower_value(*rhs),
        },
        Instruction::Call { dst, callee, args } => BytecodeInstruction::Call {
            dst: dst.map(lower_value),
            callee: match callee {
                IrCallTarget::SourceFunction(contract) => {
                    let target = context
                        .program
                        .expect("linked source program")
                        .function(&crate::module::function::FunctionInstance {
                            declaration: contract.declaration.clone(),
                            arguments: contract.arguments.clone(),
                        })
                        .expect("verified source binding");
                    CallTarget::ModuleFunction {
                        module: super::ModuleRef::new(target.module),
                        function: FunctionRef::new(target.function.index()),
                    }
                }
                IrCallTarget::Function(id) => CallTarget::Function(FunctionRef::new(id.index())),
                IrCallTarget::InterfaceMethod(contract) => CallTarget::InterfaceMethod {
                    module: context.owner_ref(&contract.interface.declaration.module),
                    interface: contract.interface.clone(),
                    method_slot: contract.method_slot,
                },
                IrCallTarget::HostFunction(declaration) => {
                    CallTarget::HostFunction(context.host_import(declaration))
                }
                IrCallTarget::Value(value) => CallTarget::Register(lower_value(*value)),
                IrCallTarget::Closure {
                    value,
                    params,
                    return_type,
                } => CallTarget::ClosureRegister {
                    register: lower_value(*value),
                    params: params.clone(),
                    return_type: *return_type,
                },
                IrCallTarget::StandardIntrinsic(intrinsic) => {
                    CallTarget::StandardIntrinsic(*intrinsic)
                }
                IrCallTarget::RuntimeHelper(helper) => {
                    CallTarget::RuntimeHelper(lower_runtime_helper(helper))
                }
            },
            args: args.iter().map(|arg| lower_value(*arg)).collect(),
        },
        Instruction::BeginIteration { collection } => BytecodeInstruction::BeginIteration {
            collection: lower_value(*collection),
        },
        Instruction::EndIteration => BytecodeInstruction::EndIteration,
        Instruction::MakeTuple { dst, elements } => BytecodeInstruction::MakeTuple {
            dst: lower_value(*dst),
            elements: elements
                .iter()
                .map(|element| lower_value(*element))
                .collect(),
        },
        Instruction::MakeArray { dst, elements } => BytecodeInstruction::MakeArray {
            dst: lower_value(*dst),
            elements: elements
                .iter()
                .map(|element| lower_value(*element))
                .collect(),
        },
        Instruction::MakeClosure {
            dst,
            function,
            captures,
        } => BytecodeInstruction::MakeClosure {
            dst: lower_value(*dst),
            function: super::FunctionRef::new(function.index()),
            captures: captures.iter().map(|value| lower_value(*value)).collect(),
        },
        Instruction::MakeCell { dst, value } => BytecodeInstruction::MakeCell {
            dst: lower_value(*dst),
            value: lower_value(*value),
        },
        Instruction::ReadCell { dst, cell } => BytecodeInstruction::ReadCell {
            dst: lower_value(*dst),
            cell: lower_value(*cell),
        },
        Instruction::WriteCell { cell, value } => BytecodeInstruction::WriteCell {
            cell: lower_value(*cell),
            value: lower_value(*value),
        },
        Instruction::UpcastInterface {
            dst,
            value,
            source,
            target,
        } => BytecodeInstruction::UpcastInterface {
            dst: lower_value(*dst),
            value: lower_value(*value),
            source: source.clone(),
            target: target.clone(),
        },
        Instruction::MakeInterface {
            dst,
            value,
            implementation,
            arguments,
        } => {
            let (module, implementation) = context.interface_ref(implementation, arguments);
            BytecodeInstruction::MakeInterface {
                dst: lower_value(*dst),
                value: lower_value(*value),
                module,
                implementation,
            }
        }
        Instruction::MapResultError {
            dst,
            original,
            error,
            ty,
        } => BytecodeInstruction::MapResultError {
            dst: lower_value(*dst),
            original: lower_value(*original),
            error: lower_value(*error),
            ty: ty.clone(),
        },
        Instruction::Cursor { dst, value, ty, op } => BytecodeInstruction::Cursor {
            dst: lower_value(*dst),
            value: value.map(lower_value),
            ty: ty.clone(),
            op: *op,
        },
        Instruction::StandardEnum { dst, value, ty, op } => BytecodeInstruction::StandardEnum {
            dst: lower_value(*dst),
            value: value.map(lower_value),
            ty: ty.clone(),
            op: *op,
        },
        Instruction::MakeEnum {
            dst,
            enumeration,
            variant,
            fields,
        } => BytecodeInstruction::MakeEnum {
            dst: lower_value(*dst),
            enumeration: super::EnumId::new(
                context
                    .enumerations
                    .iter()
                    .position(|layout| {
                        layout.declaration == enumeration.declaration
                            && layout.arguments == enumeration.arguments
                    })
                    .expect("verified enum layout"),
            ),
            variant: *variant as u32,
            fields: fields.iter().map(|value| lower_value(*value)).collect(),
        },
        Instruction::TestEnumVariant {
            dst,
            value,
            enumeration,
            variant,
        } => BytecodeInstruction::TestEnumVariant {
            dst: lower_value(*dst),
            value: lower_value(*value),
            enumeration: super::EnumId::new(
                context
                    .enumerations
                    .iter()
                    .position(|layout| {
                        layout.declaration == enumeration.declaration
                            && layout.arguments == enumeration.arguments
                    })
                    .expect("verified enum layout"),
            ),
            variant: *variant as u32,
        },
        Instruction::ReadEnumPayload {
            dst,
            value,
            enumeration,
            variant,
            index,
        } => BytecodeInstruction::ReadEnumPayload {
            dst: lower_value(*dst),
            value: lower_value(*value),
            enumeration: super::EnumId::new(
                context
                    .enumerations
                    .iter()
                    .position(|layout| {
                        layout.declaration == enumeration.declaration
                            && layout.arguments == enumeration.arguments
                    })
                    .expect("verified enum layout"),
            ),
            variant: *variant as u32,
            index: *index as u32,
        },
        Instruction::MakeStruct {
            dst,
            structure,
            fields,
        } => {
            let mut ordered = fields.iter().collect::<Vec<_>>();
            ordered.sort_by_key(|field| field.slot);
            BytecodeInstruction::MakeStruct {
                dst: lower_value(*dst),
                structure: context.structure_id(structure),
                fields: ordered
                    .into_iter()
                    .map(|field| lower_value(field.value))
                    .collect(),
            }
        }
        Instruction::ReadAggregateField { dst, base, field } => {
            BytecodeInstruction::ReadAggregateField {
                dst: lower_value(*dst),
                base: lower_value(*base),
                field: context.field_ref(field),
            }
        }
        Instruction::WriteAggregateField { base, field, value } => {
            BytecodeInstruction::WriteAggregateField {
                base: lower_value(*base),
                field: context.field_ref(field),
                value: lower_value(*value),
            }
        }
        Instruction::ReadAggregateIndex { dst, base, index } => {
            BytecodeInstruction::ReadAggregateIndex {
                dst: lower_value(*dst),
                base: lower_value(*base),
                index: lower_value(*index),
            }
        }
        Instruction::WriteAggregateIndex { base, index, value } => {
            BytecodeInstruction::WriteAggregateIndex {
                base: lower_value(*base),
                index: lower_value(*index),
                value: lower_value(*value),
            }
        }
        Instruction::ReadPath {
            dst,
            root_or_view,
            path,
            dynamic_args,
        } => BytecodeInstruction::ReadPath {
            dst: lower_value(*dst),
            root_or_view: lower_value(*root_or_view),
            path: context.path_id(path),
            dynamic_args: dynamic_args.iter().map(|arg| lower_value(*arg)).collect(),
        },
        Instruction::SetPath {
            root_or_view,
            path,
            dynamic_args,
            value,
        } => BytecodeInstruction::SetPath {
            root_or_view: lower_value(*root_or_view),
            path: context.path_id(path),
            dynamic_args: dynamic_args.iter().map(|arg| lower_value(*arg)).collect(),
            value: lower_value(*value),
        },
        Instruction::ModifyPath {
            dst,
            root_or_view,
            path,
            dynamic_args,
            op,
            value,
        } => BytecodeInstruction::ModifyPath {
            dst: dst.map(lower_value),
            root_or_view: lower_value(*root_or_view),
            path: context.path_id(path),
            dynamic_args: dynamic_args.iter().map(|arg| lower_value(*arg)).collect(),
            op: lower_binary_op(*op),
            value: lower_value(*value),
        },
        Instruction::MakePathView {
            dst,
            root_or_view,
            path,
            dynamic_args,
        } => BytecodeInstruction::MakePathView {
            dst: lower_value(*dst),
            root_or_view: lower_value(*root_or_view),
            path: context.path_id(path),
            dynamic_args: dynamic_args.iter().map(|arg| lower_value(*arg)).collect(),
        },
    }
}

fn lower_terminator(
    terminator: &Terminator,
    block_offsets: &HashMap<BlockId, JumpTarget>,
) -> Result<BytecodeInstruction, BytecodeLoweringError> {
    Ok(match terminator {
        Terminator::Return(value) => BytecodeInstruction::Return(value.map(lower_value)),
        Terminator::Jump(target) => BytecodeInstruction::Jump {
            target: lower_jump(*target, block_offsets)?,
        },
        Terminator::Branch {
            cond,
            then_block,
            else_block,
        } => BytecodeInstruction::Branch {
            cond: lower_value(*cond),
            then_target: lower_jump(*then_block, block_offsets)?,
            else_target: lower_jump(*else_block, block_offsets)?,
        },
        Terminator::Unreachable => BytecodeInstruction::Unreachable,
    })
}

fn lower_constant(constant: &Constant) -> ConstantOperand {
    match constant {
        Constant::Unit => ConstantOperand::Unit,
        Constant::Bool(value) => ConstantOperand::Bool(*value),
        Constant::I32(value) => ConstantOperand::I32(*value),
        Constant::I64(value) => ConstantOperand::I64(*value),
        Constant::F32(value) => ConstantOperand::F32(*value),
        Constant::Str(value) => ConstantOperand::Str(value.clone()),
    }
}

fn lower_binary_op(op: IrBinaryOp) -> BinaryOp {
    match op {
        IrBinaryOp::Add => BinaryOp::Add,
        IrBinaryOp::Sub => BinaryOp::Sub,
        IrBinaryOp::Mul => BinaryOp::Mul,
        IrBinaryOp::Div => BinaryOp::Div,
        IrBinaryOp::Rem => BinaryOp::Rem,
        IrBinaryOp::Eq => BinaryOp::Eq,
        IrBinaryOp::NotEq => BinaryOp::NotEq,
        IrBinaryOp::IdentityEq => BinaryOp::IdentityEq,
        IrBinaryOp::IdentityNotEq => BinaryOp::IdentityNotEq,
        IrBinaryOp::Lt => BinaryOp::Lt,
        IrBinaryOp::Gt => BinaryOp::Gt,
        IrBinaryOp::Le => BinaryOp::Le,
        IrBinaryOp::Ge => BinaryOp::Ge,
        IrBinaryOp::AndAnd | IrBinaryOp::OrOr => {
            unreachable!("short-circuit ops should be lowered into branches before bytecode")
        }
    }
}

fn lower_runtime_helper(helper: &IrRuntimeHelper) -> RuntimeHelper {
    match helper {
        IrRuntimeHelper::ReflectTypeOf => RuntimeHelper::ReflectTypeOf,
        IrRuntimeHelper::ReflectGetField(name) => RuntimeHelper::ReflectGetField(name.clone()),
        IrRuntimeHelper::ReflectSetField(name) => RuntimeHelper::ReflectSetField(name.clone()),
        IrRuntimeHelper::ReflectSetIndex => RuntimeHelper::ReflectSetIndex,
        IrRuntimeHelper::DynamicCall => RuntimeHelper::DynamicCall,
    }
}

fn lower_temp(temp: TempId) -> Register {
    Register::new(temp.index())
}

fn lower_value(value: IrValue) -> Register {
    lower_temp(value.temp)
}

fn lower_local(local: LocalId) -> LocalSlot {
    LocalSlot::new(local.index())
}

fn lower_module_slot(slot: ModuleSlotId) -> ModuleSlot {
    ModuleSlot::new(slot.index())
}

fn lower_jump(
    block: BlockId,
    block_offsets: &HashMap<BlockId, JumpTarget>,
) -> Result<JumpTarget, BytecodeLoweringError> {
    block_offsets
        .get(&block)
        .copied()
        .ok_or(BytecodeLoweringError::InvalidBranchTarget(block))
}

fn emission_order(function: &IrFunction) -> impl Iterator<Item = (usize, &BasicBlock)> {
    std::iter::once((
        function.entry.index(),
        &function.blocks[function.entry.index()],
    ))
    .chain(
        function
            .blocks
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != function.entry.index()),
    )
}

pub fn lower_program_to_bytecode(
    program: &crate::program::VerifiedIrProgram,
) -> Result<super::BytecodeProgram, BytecodeLoweringError> {
    let indices = program
        .modules()
        .iter()
        .enumerate()
        .map(|(index, module)| (&module.identity, super::ModuleRef::new(index)))
        .collect::<HashMap<_, _>>();
    let modules = program
        .modules()
        .iter()
        .map(|module| {
            lower_linked_module(
                module,
                Some(program),
                module
                    .dependencies
                    .iter()
                    .map(|identity| indices[identity])
                    .collect(),
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let bytecode = super::BytecodeProgram {
        root: indices[program.root()],
        modules,
    };
    super::verify_program(&bytecode).map_err(BytecodeLoweringError::Verification)?;
    Ok(bytecode)
}

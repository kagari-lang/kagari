mod debug;
mod defaults;
mod interfaces;
mod views;
use crate::bytecode::{
    debug::collect_debug_metadata,
    interfaces::{collect_interface_tables, interface_instances},
};
use kagari_abi::{
    ids::FunctionRef,
    layout::{EnumLayout, StructLayout},
    native_import::NativeImport,
    operations::{BinaryOp as MirBinaryOp, UnaryOp as MirUnaryOp},
    representation::ValueType,
    types::{AbiType, ConcreteFunctionIdentity, NominalAbiType},
};
use kagari_bytecode::{
    instruction::{
        BinaryOp, BytecodeInstruction, CallTarget, ConstantOperand, EnumId, FieldRef,
        InterfaceTableRef, JumpTarget, LocalSlot, ModuleSlot, NativeImportId, PathId, Register,
        RuntimeHelper, StructId, UnaryOp,
    },
    module::{
        BytecodeFunction, BytecodeModule, BytecodeModuleSlot, FunctionMetadata, FunctionRecord,
        PathRecord, RootSlotLayout,
    },
    program::{BytecodeProgram, ModuleRef, verify_program},
    verifier::{BytecodeVerificationError, verify_module},
};
use kagari_common::{
    host_interface::HostInterface,
    identity::{DefinitionId, ModuleIdentity},
    span::Span,
};
use kagari_mir::{
    analysis::FunctionAnalysis,
    function::{BasicBlock, MirFunction},
    ids::{BlockId, LocalId, ModuleSlotId, TempId},
    instruction::{
        AggregateFieldRef, CallTarget as MirCallTarget, Constant, Instruction, MirValue, PathRef,
        RuntimeHelper as MirRuntimeHelper, Terminator,
    },
    program::VerifiedMirProgram,
    verify::VerifiedMirModule,
};
use std::{collections::HashMap, slice};

#[derive(Debug)]
pub enum BytecodeLoweringError {
    UnlinkedSourceModules,
    InvalidBranchTarget(BlockId),
    Verification(BytecodeVerificationError),
    InvalidNativeInterface,
}

pub fn lower_to_bytecode(ir: &VerifiedMirModule) -> Result<BytecodeModule, BytecodeLoweringError> {
    if !ir.dependencies.is_empty()
        || ir
            .functions
            .iter()
            .flat_map(|f| &f.blocks)
            .flat_map(|b| &b.instructions)
            .any(|i| match i {
                Instruction::Call {
                    callee: MirCallTarget::SourceFunction(_),
                    ..
                } => true,
                Instruction::Call {
                    callee: MirCallTarget::InterfaceMethod(contract),
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
    let mut module = lower_linked_module(ir, None, Vec::new())?;
    views::populate(slice::from_mut(&mut module))?;
    verify_module(&module).map_err(BytecodeLoweringError::Verification)?;
    Ok(module)
}

fn lower_linked_module(
    ir: &VerifiedMirModule,
    program: Option<&VerifiedMirProgram>,
    dependencies: Vec<ModuleRef>,
) -> Result<BytecodeModule, BytecodeLoweringError> {
    let mut context = BytecodeLoweringContext {
        program,
        structures: &ir.structures,
        enumerations: &ir.enumerations,
        identity: Some(&ir.identity),
        ir: Some(ir),
        host_interface: HostInterface {
            paths: vec![],
            types: ir.host_types.clone(),
            functions: Vec::new(),
        },
        ..Default::default()
    };
    context.native_imports = ir.native_targets.clone();
    let functions = ir
        .functions
        .iter()
        .map(|function| {
            lower_function(
                function,
                ir.analysis(function.id).expect("sealed function facts"),
                &mut context,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let interface_tables = collect_interface_tables(ir, program, &mut context.native_imports)?;
    let mut module = BytecodeModule {
        dependencies,
        host_interface: context.host_interface,
        native_imports: context.native_imports,
        native_declarations: ir.abi.native_declarations.clone(),
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
        interface_tables,
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

#[derive(Debug, Default)]
struct BytecodeLoweringContext<'a> {
    program: Option<&'a VerifiedMirProgram>,
    structures: &'a [StructLayout],
    enumerations: &'a [EnumLayout],
    identity: Option<&'a ModuleIdentity>,
    ir: Option<&'a VerifiedMirModule>,
    interface_tables: HashMap<ModuleIdentity, Vec<ConcreteFunctionIdentity>>,
    host_interface: HostInterface,
    native_imports: Vec<NativeImport>,
    paths: Vec<PathRecord>,
}

impl BytecodeLoweringContext<'_> {
    fn owner_ref(&self, owner: &ModuleIdentity) -> ModuleRef {
        if let Some(program) = self.program {
            let index = program
                .modules()
                .iter()
                .position(|module| &module.identity == owner)
                .expect("verified interface owner module");
            ModuleRef::new(index)
        } else {
            assert_eq!(self.identity.expect("lowering module identity"), owner);
            ModuleRef::new(0)
        }
    }

    fn interface_ref(
        &mut self,
        implementation: &DefinitionId,
        arguments: &[AbiType],
    ) -> (ModuleRef, InterfaceTableRef) {
        let (module, owner) = if let Some(program) = self.program {
            program
                .modules()
                .iter()
                .enumerate()
                .find(|(_, owner)| owner.identity == implementation.module)
                .map(|(index, owner)| (ModuleRef::new(index), owner))
                .expect("verified interface owner")
        } else {
            (ModuleRef::new(0), self.ir.expect("lowering module"))
        };
        let tables = self
            .interface_tables
            .entry(owner.identity.clone())
            .or_insert_with(|| interface_instances(owner, self.program));
        let table = tables
            .iter()
            .position(|table| table.declaration == *implementation && table.arguments == arguments)
            .expect("verified interface instance");
        (module, InterfaceTableRef::new(table))
    }

    fn native_import(&mut self, contract: &NativeImport) -> NativeImportId {
        if let Some(host) = &contract.host
            && !self
                .host_interface
                .functions
                .iter()
                .any(|existing| existing.id == host.id)
        {
            self.host_interface.functions.push(host.clone());
        }
        if let Some(index) = self
            .native_imports
            .iter()
            .position(|existing| existing == contract)
        {
            return NativeImportId::new(index);
        }
        let id = NativeImportId::new(self.native_imports.len());
        self.native_imports.push(contract.clone());
        id
    }

    fn structure_id(&self, id: &NominalAbiType) -> StructId {
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
    function: &MirFunction,
    analysis: &FunctionAnalysis,
    context: &mut BytecodeLoweringContext,
) -> Result<BytecodeFunction, BytecodeLoweringError> {
    let block_offsets = compute_block_offsets(function, analysis);
    let mut instructions = Vec::with_capacity(
        function
            .blocks
            .iter()
            .map(|block| block.instructions.len() + usize::from(block.terminator.is_some()))
            .sum(),
    );
    let mut instruction_spans = Vec::with_capacity(instructions.capacity());
    for (_, block) in function.emission_order() {
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
            analysis,
            &instruction_spans,
            function
                .debug
                .source_module
                .as_ref()
                .map(|owner| context.owner_ref(owner)),
        ),
    };

    Ok(BytecodeFunction {
        id: FunctionRef::new(function.id.index()),
        identity: Some(function.instance.clone()),
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

fn push_target(targets: &mut Vec<JumpTarget>, target: JumpTarget) {
    if !targets.contains(&target) {
        targets.push(target);
    }
}

fn compute_block_offsets(
    function: &MirFunction,
    analysis: &FunctionAnalysis,
) -> HashMap<BlockId, JumpTarget> {
    function
        .blocks
        .iter()
        .enumerate()
        .map(|(index, _)| {
            let id = BlockId::new(index);
            (
                id,
                JumpTarget::new(
                    analysis
                        .block(id)
                        .expect("sealed block facts")
                        .start_offset(),
                ),
            )
        })
        .collect()
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
                MirUnaryOp::Neg => UnaryOp::Neg,
                MirUnaryOp::Not => UnaryOp::Not,
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
                MirCallTarget::SourceFunction(contract) => {
                    let target = context
                        .program
                        .expect("linked source program")
                        .function(&ConcreteFunctionIdentity {
                            declaration: contract.declaration.clone(),
                            arguments: contract.arguments.clone(),
                        })
                        .expect("verified source binding");
                    CallTarget::ModuleFunction {
                        module: ModuleRef::new(target.module),
                        function: FunctionRef::new(target.function.index()),
                    }
                }
                MirCallTarget::Function(id) => CallTarget::Function(FunctionRef::new(id.index())),
                MirCallTarget::InterfaceMethod(contract) => CallTarget::InterfaceMethod {
                    module: context.owner_ref(&contract.interface.declaration.module),
                    interface: contract.interface.clone(),
                    method_slot: contract.method_slot,
                },
                MirCallTarget::Native(contract) => {
                    CallTarget::Native(context.native_import(contract))
                }

                MirCallTarget::Value(value) => CallTarget::Register(lower_value(*value)),
                MirCallTarget::Closure {
                    value,
                    params,
                    return_type,
                } => CallTarget::ClosureRegister {
                    register: lower_value(*value),
                    params: params.clone(),
                    return_type: *return_type,
                },
                MirCallTarget::RuntimePrimitive(intrinsic) => {
                    CallTarget::RuntimePrimitive(*intrinsic)
                }
                MirCallTarget::RuntimeHelper(helper) => {
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
        Instruction::RangeBound {
            dst,
            value,
            range,
            bound,
            upper,
        } => BytecodeInstruction::RangeBound {
            dst: lower_value(*dst),
            value: lower_value(*value),
            range: range.clone(),
            bound: bound.clone(),
            upper: *upper,
        },
        Instruction::MakeRange {
            dst,
            start,
            end,
            ty,
        } => BytecodeInstruction::MakeRange {
            dst: lower_value(*dst),
            start: start.map(lower_value),
            end: end.map(lower_value),
            ty: ty.clone(),
        },
        Instruction::RepeatArray {
            dst,
            element,
            value,
            count,
        } => BytecodeInstruction::RepeatArray {
            element: element.clone(),
            dst: lower_value(*dst),
            value: lower_value(*value),
            count: lower_value(*count),
        },
        Instruction::MakeArray {
            dst,
            element,
            elements,
        } => BytecodeInstruction::MakeArray {
            element: element.clone(),
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
            function: FunctionRef::new(function.index()),
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
        Instruction::Convert {
            dst,
            src,
            conversion,
        } => BytecodeInstruction::Convert {
            dst: lower_value(*dst),
            src: lower_value(*src),
            conversion: *conversion,
        },
        Instruction::Numeric {
            dst,
            operation,
            lhs,
            rhs,
        } => BytecodeInstruction::Numeric {
            dst: lower_value(*dst),
            operation: *operation,
            lhs: lower_value(*lhs),
            rhs: rhs.map(lower_value),
        },
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
        Instruction::Iter { dst, value, ty, op } => BytecodeInstruction::Iter {
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
            enumeration: EnumId::new(
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
            enumeration: EnumId::new(
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
            enumeration: EnumId::new(
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
        Constant::F64(value) => ConstantOperand::F64(*value),
        Constant::U64(value) => ConstantOperand::U64(*value),
        Constant::Str(value) => ConstantOperand::Str(value.clone()),
    }
}

fn lower_binary_op(op: MirBinaryOp) -> BinaryOp {
    match op {
        MirBinaryOp::Numeric(op) => BinaryOp::Numeric(op),
        MirBinaryOp::Add => BinaryOp::Add,
        MirBinaryOp::Sub => BinaryOp::Sub,
        MirBinaryOp::Mul => BinaryOp::Mul,
        MirBinaryOp::Div => BinaryOp::Div,
        MirBinaryOp::Rem => BinaryOp::Rem,
        MirBinaryOp::Eq => BinaryOp::Eq,
        MirBinaryOp::NotEq => BinaryOp::NotEq,
        MirBinaryOp::IdentityEq => BinaryOp::IdentityEq,
        MirBinaryOp::IdentityNotEq => BinaryOp::IdentityNotEq,
        MirBinaryOp::Lt => BinaryOp::Lt,
        MirBinaryOp::Gt => BinaryOp::Gt,
        MirBinaryOp::Le => BinaryOp::Le,
        MirBinaryOp::Ge => BinaryOp::Ge,
        MirBinaryOp::AndAnd | MirBinaryOp::OrOr => {
            unreachable!("short-circuit ops should be lowered into branches before bytecode")
        }
    }
}

fn lower_runtime_helper(helper: &MirRuntimeHelper) -> RuntimeHelper {
    match helper {
        MirRuntimeHelper::ReflectTypeOf => RuntimeHelper::ReflectTypeOf,
        MirRuntimeHelper::ReflectGetField(name) => RuntimeHelper::ReflectGetField(name.clone()),
        MirRuntimeHelper::ReflectSetField(name) => RuntimeHelper::ReflectSetField(name.clone()),
        MirRuntimeHelper::ReflectSetIndex => RuntimeHelper::ReflectSetIndex,
        MirRuntimeHelper::DynamicCall => RuntimeHelper::DynamicCall,
    }
}

fn lower_temp(temp: TempId) -> Register {
    Register::new(temp.index())
}

fn lower_value(value: MirValue) -> Register {
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

pub fn lower_program_to_bytecode(
    program: &VerifiedMirProgram,
) -> Result<BytecodeProgram, BytecodeLoweringError> {
    let indices = program
        .modules()
        .iter()
        .enumerate()
        .map(|(index, module)| (&module.identity, ModuleRef::new(index)))
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
    let mut bytecode = BytecodeProgram {
        root: indices[program.root()],
        modules,
    };
    views::populate(&mut bytecode.modules)?;
    verify_program(&bytecode).map_err(BytecodeLoweringError::Verification)?;
    Ok(bytecode)
}

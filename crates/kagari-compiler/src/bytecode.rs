use crate::bytecode::{
    debug::collect_debug_metadata,
    interfaces::{collect_interface_tables, interface_instances},
};
use kagari_abi::representation::ValueType;
use kagari_bytecode::instruction::ConstructionTarget as BytecodeConstruction;
use kagari_bytecode::{
    artifact::ArtifactValidationError,
    instruction::{
        BinaryOp, BytecodeInstruction, CallTarget, ConstantId, ConstantOperand, EnumId, FieldRef,
        InterfaceTableRef, JumpTarget, ModuleSlot, NativeImportId, PathId, RuntimeHelper, Slot,
        StructId, UnaryOp,
    },
    module::{
        BytecodeFunction, BytecodeModule, BytecodeSemanticSlots, CallableTarget, FunctionMetadata,
        FunctionRecord, InstructionOrigin, PathRecord, RootSlotLayout,
    },
    program::{BytecodeProgram, ModuleRef, verified::VerifiedBytecodeProgram},
    verifier::{BytecodeVerificationError, verify_module},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{ModuleIdentity, metadata::DefinitionMetadata, table::DefinitionId},
    span::Span,
};

use kagari_contract::{
    ids::FunctionRef,
    layout::{EnumLayout, StructLayout},
    native_import::NativeImport,
    operations::{BinaryOp as MirBinaryOp, UnaryOp as MirUnaryOp},
    representation::semantic_representation,
    statics::StaticDescriptor,
    types::{ConcreteFunctionIdentity, proofs::ProofCatalog},
};
use kagari_mir::instruction::ConstructionTarget as MirConstruction;
use kagari_mir::{
    analysis::FunctionAnalysis,
    function::{BasicBlock, MirFunction},
    ids::{BlockId, LocalId, ModuleSlotId},
    instruction::{
        AggregateFieldRef, CallTarget as MirCallTarget, Constant, Instruction, MirValue, PathRef,
        RuntimeHelper as MirRuntimeHelper, Terminator,
    },
    program::VerifiedMirProgram,
    verify::VerifiedMirModule,
};
use kagari_types::{
    callable::CallableImplementation,
    declaration::path::HostPathDeclaration,
    ty::{NominalTy, Ty},
};
use std::{collections::HashMap, slice};

mod debug;
mod defaults;
mod interfaces;
mod placement;
mod views;

#[derive(Debug)]
pub enum BytecodeLoweringError {
    UnlinkedSourceModules,
    InvalidBranchTarget(BlockId),
    Verification(BytecodeVerificationError),
    ProgramValidation(ArtifactValidationError),
    InvalidNativeInterface,
}

/// Lower and verify an independent MIR module while retaining its identity scope.
///
/// The returned editable metadata carries identities, not an execution seal.
pub fn lower_to_bytecode(
    ir: &VerifiedMirModule,
) -> Result<DefinitionMetadata<BytecodeModule>, BytecodeLoweringError> {
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
                } => {
                    ir.definitions()
                        .resolve(contract.interface.declaration)
                        .expect("verified interface identity")
                        .module()
                        != &ir.identity
                }
                Instruction::Call {
                    callee: MirCallTarget::Shared(contract),
                    ..
                } => {
                    ir.definitions()
                        .resolve(contract.instance.declaration)
                        .expect("verified callable identity")
                        .module()
                        != &ir.identity
                }
                Instruction::MakeInterface {
                    target: MirConstruction::Declared { implementation, .. },
                    ..
                } => {
                    ir.definitions()
                        .resolve(*implementation)
                        .expect("verified implementation identity")
                        .module()
                        != &ir.identity
                }
                _ => false,
            })
    {
        return Err(BytecodeLoweringError::UnlinkedSourceModules);
    }
    let cancel = CancellationToken::default();
    let catalog = defaults::catalog(slice::from_ref(ir), ir.definitions(), &cancel)?;
    let mut module = lower_linked_module(ir, None, Vec::new(), catalog.as_ref(), &cancel)?;
    views::populate(slice::from_mut(&mut module), ir.definitions())?;
    verify_module(&module, ir.definitions(), &Default::default())
        .map_err(BytecodeLoweringError::Verification)?;
    DefinitionMetadata::checked(ir.definitions().clone(), module, &Default::default())
        .map_err(|_| BytecodeLoweringError::InvalidNativeInterface)
}

fn lower_linked_module(
    ir: &VerifiedMirModule,
    program: Option<&VerifiedMirProgram>,
    dependencies: Vec<ModuleRef>,
    catalog: Option<&ProofCatalog<'_>>,
    cancel: &CancellationToken,
) -> Result<BytecodeModule, BytecodeLoweringError> {
    let mut context = BytecodeLoweringContext {
        program,
        structures: &ir.structures,
        enumerations: &ir.enumerations,
        identity: Some(&ir.identity),
        ir: Some(ir),
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
    let interface_tables =
        collect_interface_tables(ir, program, &mut context.native_imports, catalog, cancel)?;
    let mut module = BytecodeModule {
        dependencies,
        external_types: ir.external_types.clone(),
        path_declarations: context.path_declarations,
        native_imports: context.native_imports,
        native_declarations: ir.abi.native_declarations.clone(),
        identity: ir.identity.clone(),
        source_name: ir.source_name.clone(),
        module_slots: ir
            .module_slots
            .iter()
            .map(|slot| StaticDescriptor {
                contract: slot.contract.clone(),
                initializer: FunctionRef::new(slot.initializer.index()),
            })
            .collect(),
        constants: context.constants,
        types: Vec::new(),
        structures: ir.structures.clone(),
        enumerations: ir.enumerations.clone(),
        interface_tables,
        paths: context.paths,
        function_table: Vec::new(),
        public_items: ir.abi.public_items.clone(),
        trait_references: ir.abi.trait_references.clone(),
        trait_contracts: ir.abi.trait_contracts.clone(),
        functions,
    };
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
    path_declarations: Vec<HostPathDeclaration>,
    native_imports: Vec<NativeImport>,
    paths: Vec<PathRecord>,
    constants: Vec<ConstantOperand>,
    constant_ids: HashMap<ConstantOperand, ConstantId>,
    local_count: usize,
}

impl BytecodeLoweringContext<'_> {
    fn value(&self, value: MirValue) -> Slot {
        Slot::new(self.local_count + value.temp.index())
    }

    fn constant_id(&mut self, value: ConstantOperand) -> ConstantId {
        if let Some(id) = self.constant_ids.get(&value) {
            return *id;
        }
        let id = ConstantId::new(self.constants.len());
        self.constants.push(value.clone());
        self.constant_ids.insert(value, id);
        id
    }

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
        arguments: &[Ty],
    ) -> (ModuleRef, InterfaceTableRef) {
        let (module, owner) = if let Some(program) = self.program {
            program
                .modules()
                .iter()
                .enumerate()
                .find(|(_, owner)| {
                    &owner.identity
                        == self
                            .ir
                            .expect("lowering owner")
                            .definitions()
                            .resolve(*implementation)
                            .expect("verified implementation")
                            .module()
                })
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
            .position(|table| {
                table.declaration == *implementation
                    && table.arguments
                        == interfaces::table_arguments(owner, implementation, arguments)
            })
            .expect("verified interface instance");
        (module, InterfaceTableRef::new(table))
    }

    fn native_import(&mut self, contract: &NativeImport) -> NativeImportId {
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

    fn structure_id(&self, id: &NominalTy) -> StructId {
        StructId::new(
            self.structures
                .iter()
                .position(|layout| {
                    layout.declaration == id.declaration && layout.accepts(&id.arguments)
                })
                .expect("verified struct layout"),
        )
    }

    fn field_ref(&self, field: &AggregateFieldRef) -> FieldRef {
        FieldRef {
            structure: self.structure_id(&field.owner),
            arguments: field.owner.arguments.clone(),
            slot: field.slot as u32,
            reference: field.reference,
        }
    }

    fn path_id(&mut self, path: &PathRef) -> PathId {
        if let Some(declaration) = &path.declaration
            && !self.path_declarations.contains(declaration)
        {
            self.path_declarations.push(declaration.clone());
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
    context.local_count = function.locals.len();
    let block_offsets = compute_block_offsets(function, analysis);
    let mut instructions = Vec::new();
    let mut instruction_spans = Vec::new();
    for (_, block) in function.emission_order() {
        lower_block(
            block,
            &block_offsets,
            context,
            &mut instructions,
            &mut instruction_spans,
        )?;
    }
    let mut semantic = BytecodeSemanticSlots {
        generic: function.semantic.generic.clone(),
        protocol_adapter: function.semantic.protocol_adapter.clone(),
        params: function.semantic.params.clone(),
        result: function.semantic.result.clone(),
        slots: function.semantic.locals.clone(),
    };
    for (index, ty) in &function.semantic.registers {
        semantic
            .slots
            .insert(function.locals.len() + index, ty.clone());
    }
    placement::project_scalar_facts(function, &mut semantic);
    let slots = function
        .locals
        .iter()
        .map(|local| local.ty)
        .chain(function.temps.iter().map(|temp| temp.ty))
        .collect::<Vec<_>>();
    let debug = collect_debug_metadata(
        function,
        analysis,
        &instruction_spans,
        function
            .debug
            .source_module
            .as_ref()
            .map(|owner| context.owner_ref(owner)),
    );
    let metadata = FunctionMetadata {
        semantic,
        params: function.params.iter().map(|param| param.ty).collect(),
        parameter_slots: function
            .params
            .iter()
            .map(|param| lower_local(param.local))
            .collect(),
        return_type: function.return_type,
        roots: RootSlotLayout::from_types(&slots),
        slots,
        origins: instruction_spans
            .iter()
            .enumerate()
            .map(|(instruction, &span)| InstructionOrigin {
                line: debug.line_table[instruction].line,
                column: debug.line_table[instruction].column,
                instruction,
                span,
            })
            .collect(),
        instruction_origins: Vec::new(),
        control_flow_targets: collect_control_flow_targets(&instructions),
        effects: function.effects,
        debug,
    };
    let mut bytecode = BytecodeFunction {
        id: FunctionRef::new(function.id.index()),
        identity: Some(function.instance.clone()),
        name: function.name.clone(),
        parameter_count: u16::try_from(function.params.len()).expect("verified parameter count"),
        slot_count: u32::try_from(metadata.slots.len()).expect("bounded MIR slots"),
        metadata,
        instructions,
    };
    placement::select(function, analysis, &mut bytecode, &context.constants);
    Ok(bytecode)
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

fn collect_type_table(module: &BytecodeModule) -> Vec<ValueType> {
    let mut types = Vec::new();
    for slot in &module.module_slots {
        push_type(&mut types, semantic_representation(&slot.contract.ty));
    }
    for layout in &module.structures {
        for field in &layout.fields {
            push_type(&mut types, semantic_representation(&field.ty));
        }
    }
    for layout in &module.enumerations {
        for variant in &layout.variants {
            for ty in &variant.payload {
                push_type(&mut types, semantic_representation(ty));
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
            .chain(&function.metadata.slots)
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
            }
            | BytecodeInstruction::PredicateBranch {
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
        out.push(lower_terminator(terminator, block_offsets, context)?);
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
            dst: context.value(*dst),
            constant: context.constant_id(lower_constant(constant)),
        },
        Instruction::LoadLocal { dst, local } => BytecodeInstruction::Move {
            dst: context.value(*dst),
            src: lower_local(*local),
        },
        Instruction::LoadModule { dst, slot } => BytecodeInstruction::LoadModule {
            dst: context.value(*dst),
            slot: lower_module_slot(*slot),
        },
        Instruction::StoreLocal { local, src } => BytecodeInstruction::Move {
            dst: lower_local(*local),
            src: context.value(*src),
        },
        Instruction::StoreModule { slot, src } => BytecodeInstruction::StoreModule {
            slot: lower_module_slot(*slot),
            src: context.value(*src),
        },
        Instruction::Move { dst, src } => BytecodeInstruction::Move {
            dst: context.value(*dst),
            src: context.value(*src),
        },
        Instruction::Unary { dst, op, operand } => BytecodeInstruction::Unary {
            dst: context.value(*dst),
            op: match op {
                MirUnaryOp::Neg => UnaryOp::Neg,
                MirUnaryOp::Not => UnaryOp::Not,
            },
            operand: context.value(*operand),
        },
        Instruction::Binary { dst, op, lhs, rhs } => BytecodeInstruction::Binary {
            dst: context.value(*dst),
            op: lower_binary_op(*op),
            lhs: context.value(*lhs),
            rhs: context.value(*rhs),
        },
        Instruction::Call { dst, callee, args } => BytecodeInstruction::Call {
            dst: dst.map(|value| context.value(value)),
            callee: match callee {
                MirCallTarget::SourceFunction(contract) => {
                    let target = context
                        .program
                        .expect("linked source program")
                        .function(&ConcreteFunctionIdentity {
                            declaration: contract.declaration,
                            arguments: contract.arguments.clone(),
                        })
                        .expect("verified source binding");
                    CallTarget::ModuleFunction {
                        module: ModuleRef::new(target.module),
                        function: FunctionRef::new(target.function.index()),
                    }
                }
                MirCallTarget::Shared(contract) => {
                    let ir = context.ir.expect("shared call owner");
                    let (module, target) = match &contract.implementation {
                        CallableImplementation::Script => {
                            if let Some(program) = context.program {
                                let target = program
                                    .function(&contract.instance)
                                    .expect("verified shared function");
                                (
                                    ModuleRef::new(target.module),
                                    CallableTarget::Script(FunctionRef::new(
                                        target.function.index(),
                                    )),
                                )
                            } else {
                                let target = ir
                                    .functions
                                    .iter()
                                    .find(|function| function.instance == contract.instance)
                                    .expect("verified shared function");
                                (
                                    ModuleRef::new(0),
                                    CallableTarget::Script(FunctionRef::new(target.id.index())),
                                )
                            }
                        }
                        CallableImplementation::Native(_) => {
                            let import = ir
                                .native_targets
                                .iter()
                                .find(|import| import.instance == contract.instance)
                                .expect("verified shared native target");
                            (
                                context.owner_ref(&ir.identity),
                                CallableTarget::Native(context.native_import(import)),
                            )
                        }
                        _ => unreachable!("verified shared callable kind"),
                    };
                    CallTarget::Shared {
                        module,
                        target,
                        contract: contract.clone(),
                    }
                }
                MirCallTarget::Function(id) => CallTarget::Function(FunctionRef::new(id.index())),
                MirCallTarget::InterfaceMethod(contract) => CallTarget::InterfaceMethod {
                    module: context.owner_ref(
                        context
                            .ir
                            .expect("lowering module")
                            .definitions()
                            .resolve(contract.interface.declaration)
                            .expect("verified interface identity")
                            .module(),
                    ),
                    contract: contract.clone(),
                },
                MirCallTarget::Native(contract) => {
                    CallTarget::Native(context.native_import(contract))
                }

                MirCallTarget::Value(value) => CallTarget::Slot(context.value(*value)),
                MirCallTarget::Closure {
                    value,
                    params,
                    return_type,
                } => CallTarget::ClosureRegister {
                    register: context.value(*value),
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
            args: args.iter().map(|arg| context.value(*arg)).collect(),
        },
        Instruction::MakeTuple { dst, elements } => BytecodeInstruction::MakeTuple {
            dst: context.value(*dst),
            elements: elements
                .iter()
                .map(|element| context.value(*element))
                .collect(),
        },
        Instruction::RangeBound {
            dst,
            value,
            range,
            bound,
            upper,
        } => BytecodeInstruction::RangeBound {
            dst: context.value(*dst),
            value: context.value(*value),
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
            dst: context.value(*dst),
            start: start.map(|value| context.value(value)),
            end: end.map(|value| context.value(value)),
            ty: ty.clone(),
        },
        Instruction::RepeatArray {
            dst,
            element,
            value,
            count,
        } => BytecodeInstruction::RepeatArray {
            element: element.clone(),
            dst: context.value(*dst),
            value: context.value(*value),
            count: context.value(*count),
        },
        Instruction::MakeArray {
            dst,
            element,
            elements,
        } => BytecodeInstruction::MakeArray {
            element: element.clone(),
            dst: context.value(*dst),
            elements: elements
                .iter()
                .map(|element| context.value(*element))
                .collect(),
        },
        Instruction::MakeFuture {
            dst,
            function,
            arguments,
            future,
        } => BytecodeInstruction::MakeFuture {
            dst: context.value(*dst),
            function: FunctionRef::new(function.index()),
            arguments: arguments
                .iter()
                .copied()
                .map(|value| context.value(value))
                .collect(),
            future: future.clone(),
        },
        Instruction::MakeClosure {
            dst,
            function,
            captures,
        } => BytecodeInstruction::MakeClosure {
            dst: context.value(*dst),
            function: FunctionRef::new(function.index()),
            captures: captures.iter().map(|value| context.value(*value)).collect(),
        },
        Instruction::MakeCell { dst, value } => BytecodeInstruction::MakeCell {
            dst: context.value(*dst),
            value: context.value(*value),
        },
        Instruction::ReadCell { dst, cell } => BytecodeInstruction::ReadCell {
            dst: context.value(*dst),
            cell: context.value(*cell),
        },
        Instruction::WriteCell { cell, value } => BytecodeInstruction::WriteCell {
            cell: context.value(*cell),
            value: context.value(*value),
        },
        Instruction::UpcastInterface {
            dst,
            value,
            source,
            target,
        } => BytecodeInstruction::UpcastInterface {
            dst: context.value(*dst),
            value: context.value(*value),
            source: source.clone(),
            target: target.clone(),
        },
        Instruction::MakeInterface {
            dst,
            value,
            target: MirConstruction::Bound(required),
        } => BytecodeInstruction::MakeInterface {
            dst: context.value(*dst),
            value: context.value(*value),
            target: BytecodeConstruction::Bound(required.clone()),
        },
        Instruction::MakeInterface {
            dst,
            value,
            target:
                MirConstruction::Declared {
                    implementation,
                    arguments,
                },
        } => {
            let (module, implementation) = context.interface_ref(implementation, arguments);
            BytecodeInstruction::MakeInterface {
                dst: context.value(*dst),
                value: context.value(*value),
                target: BytecodeConstruction::Declared {
                    module,
                    implementation,
                    arguments: arguments.clone(),
                },
            }
        }
        Instruction::Convert {
            dst,
            src,
            conversion,
        } => BytecodeInstruction::Convert {
            dst: context.value(*dst),
            src: context.value(*src),
            conversion: *conversion,
        },
        Instruction::Numeric {
            dst,
            operation,
            lhs,
            rhs,
        } => BytecodeInstruction::Numeric {
            dst: context.value(*dst),
            operation: *operation,
            lhs: context.value(*lhs),
            rhs: rhs.map(|value| context.value(value)),
        },
        Instruction::Await { dst, value, future } => BytecodeInstruction::Await {
            dst: context.value(*dst),
            value: context.value(*value),
            future: future.clone(),
        },
        Instruction::Iter { dst, value, ty, op } => BytecodeInstruction::Iter {
            dst: context.value(*dst),
            value: value.map(|value| context.value(value)),
            ty: ty.clone(),
            op: *op,
        },

        Instruction::MakeEnum {
            dst,
            enumeration,
            variant,
            fields,
        } => BytecodeInstruction::MakeEnum {
            dst: context.value(*dst),
            enumeration: EnumId::new(
                context
                    .enumerations
                    .iter()
                    .position(|layout| {
                        layout.declaration == enumeration.declaration
                            && layout.accepts(&enumeration.arguments)
                    })
                    .expect("verified enum layout"),
            ),
            arguments: enumeration.arguments.clone(),
            variant: *variant as u32,
            fields: fields.iter().map(|value| context.value(*value)).collect(),
        },
        Instruction::TestEnumVariant {
            dst,
            value,
            enumeration,
            variant,
        } => BytecodeInstruction::TestEnumVariant {
            dst: context.value(*dst),
            value: context.value(*value),
            enumeration: EnumId::new(
                context
                    .enumerations
                    .iter()
                    .position(|layout| {
                        layout.declaration == enumeration.declaration
                            && layout.accepts(&enumeration.arguments)
                    })
                    .expect("verified enum layout"),
            ),
            arguments: enumeration.arguments.clone(),
            variant: *variant as u32,
        },
        Instruction::ReadEnumPayload {
            dst,
            value,
            enumeration,
            variant,
            index,
        } => BytecodeInstruction::ReadEnumPayload {
            dst: context.value(*dst),
            value: context.value(*value),
            enumeration: EnumId::new(
                context
                    .enumerations
                    .iter()
                    .position(|layout| {
                        layout.declaration == enumeration.declaration
                            && layout.accepts(&enumeration.arguments)
                    })
                    .expect("verified enum layout"),
            ),
            arguments: enumeration.arguments.clone(),
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
                dst: context.value(*dst),
                structure: context.structure_id(structure),
                arguments: structure.arguments.clone(),
                fields: ordered
                    .into_iter()
                    .map(|field| context.value(field.value))
                    .collect(),
            }
        }
        Instruction::ReadAggregateField { dst, base, field } => {
            BytecodeInstruction::ReadAggregateField {
                dst: context.value(*dst),
                base: context.value(*base),
                field: context.field_ref(field),
            }
        }
        Instruction::WriteAggregateField { base, field, value } => {
            BytecodeInstruction::WriteAggregateField {
                base: context.value(*base),
                field: context.field_ref(field),
                value: context.value(*value),
            }
        }
        Instruction::ReadAggregateIndex { dst, base, index } => {
            BytecodeInstruction::ReadAggregateIndex {
                dst: context.value(*dst),
                base: context.value(*base),
                index: context.value(*index),
            }
        }
        Instruction::WriteAggregateIndex { base, index, value } => {
            BytecodeInstruction::WriteAggregateIndex {
                base: context.value(*base),
                index: context.value(*index),
                value: context.value(*value),
            }
        }
        Instruction::ReadPath {
            dst,
            root_or_view,
            path,
            dynamic_args,
        } => BytecodeInstruction::ReadPath {
            dst: context.value(*dst),
            root_or_view: context.value(*root_or_view),
            path: context.path_id(path),
            dynamic_args: dynamic_args.iter().map(|arg| context.value(*arg)).collect(),
        },
        Instruction::SetPath {
            root_or_view,
            path,
            dynamic_args,
            value,
        } => BytecodeInstruction::SetPath {
            root_or_view: context.value(*root_or_view),
            path: context.path_id(path),
            dynamic_args: dynamic_args.iter().map(|arg| context.value(*arg)).collect(),
            value: context.value(*value),
        },
        Instruction::ModifyPath {
            dst,
            root_or_view,
            path,
            dynamic_args,
            op,
            value,
        } => BytecodeInstruction::ModifyPath {
            dst: dst.map(|value| context.value(value)),
            root_or_view: context.value(*root_or_view),
            path: context.path_id(path),
            dynamic_args: dynamic_args.iter().map(|arg| context.value(*arg)).collect(),
            op: lower_binary_op(*op),
            value: context.value(*value),
        },
        Instruction::MakePathView {
            dst,
            root_or_view,
            path,
            dynamic_args,
        } => BytecodeInstruction::MakePathView {
            dst: context.value(*dst),
            root_or_view: context.value(*root_or_view),
            path: context.path_id(path),
            dynamic_args: dynamic_args.iter().map(|arg| context.value(*arg)).collect(),
        },
    }
}

fn lower_terminator(
    terminator: &Terminator,
    block_offsets: &HashMap<BlockId, JumpTarget>,
    context: &BytecodeLoweringContext,
) -> Result<BytecodeInstruction, BytecodeLoweringError> {
    Ok(match terminator {
        Terminator::Return(value) => {
            BytecodeInstruction::Return(value.map(|value| context.value(value)))
        }
        Terminator::Jump(target) => BytecodeInstruction::Jump {
            target: lower_jump(*target, block_offsets)?,
        },
        Terminator::Branch {
            cond,
            then_block,
            else_block,
        } => BytecodeInstruction::Branch {
            cond: context.value(*cond),
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

fn lower_local(local: LocalId) -> Slot {
    Slot::new(local.index())
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

/// Lower linked MIR and retain complete bytecode verification evidence.
///
/// The seal owns its immutable definition scope, export targets and suspension
/// facts. Extracting editable records explicitly discards this evidence.
pub fn lower_program_to_bytecode(
    program: &VerifiedMirProgram,
    cancel: &CancellationToken,
) -> Result<VerifiedBytecodeProgram, BytecodeLoweringError> {
    let bytecode = lower_program_for_comparison(program, cancel)?;
    VerifiedBytecodeProgram::from_metadata(bytecode, cancel)
        .map_err(BytecodeLoweringError::ProgramValidation)
}

/// This unsealed result is only suitable for comparison with independently
/// verified bytecode. Public emission must verify it before returning code.
pub(crate) fn lower_program_for_comparison(
    program: &VerifiedMirProgram,
    cancel: &CancellationToken,
) -> Result<DefinitionMetadata<BytecodeProgram>, BytecodeLoweringError> {
    let catalog = defaults::catalog(program.modules(), program.definitions(), cancel)?;
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
                catalog.as_ref(),
                cancel,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut bytecode = BytecodeProgram {
        root: indices[program.root()],
        exports: program.exports().clone(),
        modules,
    };
    views::populate(&mut bytecode.modules, program.definitions())?;
    DefinitionMetadata::checked(program.definitions().clone(), bytecode, cancel)
        .map_err(|_| BytecodeLoweringError::InvalidNativeInterface)
}

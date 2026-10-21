//! Direct formatting contracts. These private fixtures deliberately do not model
//! executable programs: production entrypoints supply verified records instead.

use crate::instruction::ConstructionTarget as BytecodeConstruction;
use crate::instruction::Slot;
mod calls;
mod operations;

use crate::{
    disassembly::{
        instructions::write_instruction, presentation::DisplayContext, resolution::Resolution,
    },
    instruction::*,
    module::{BytecodeFunction, BytecodeModule, InterfaceTableRecord},
    program::{BytecodeProgram, ModuleRef},
};
use kagari_common::identity::{
    DefinitionKind, ModuleIdentity, PackageId,
    table::{DefinitionId, DefinitionTable, DefinitionTableBuilder},
};
use kagari_contract::{
    ids::FunctionRef,
    native_import::NativeImport,
    types::{ConcreteFunctionIdentity, InterfaceTable, PublicItem, TraitContract},
};
use kagari_types::{
    callable::{CallableImplementation, Signature},
    declaration::{FnDecl, NativeDeclaration, Param, TraitDef},
    reference::ReferenceAccess,
    scalar::BuiltinType,
    ty::{NominalTy, Ty},
};
use std::io::{self, Write};

struct Fixture {
    definitions: DefinitionTable,
    program: BytecodeProgram,
    declaration: DefinitionId,
    interface: DefinitionId,
    member: DefinitionId,
    table: DefinitionId,
}

impl Fixture {
    fn new() -> Self {
        let mut definitions = DefinitionTableBuilder::new().unwrap();
        let identity = ModuleIdentity {
            package: PackageId("fixture".into()),
            path: vec!["app".into()],
        };
        let root = definitions.intern_root(&identity).unwrap();
        let declaration = definitions
            .intern_child(root, DefinitionKind::Function, "entry", 0)
            .unwrap();
        let interface = definitions
            .intern_child(root, DefinitionKind::Trait, "Interface", 0)
            .unwrap();
        let member = definitions
            .intern_child(interface, DefinitionKind::Method, "method", 0)
            .unwrap();
        let table = definitions
            .intern_child(root, DefinitionKind::Impl, "table", 0)
            .unwrap();
        let signature = Signature {
            params: vec![Ty::Builtin(BuiltinType::U16)],
            result: Ty::Builtin(BuiltinType::Bool),
        };
        let declared_method = FnDecl {
            host_policy: None,
            method_policy: Default::default(),
            name: "method".into(),
            implementation: CallableImplementation::Required,
            generic_params: vec![],
            bounds: vec![],
            params: vec![Param {
                name: "self".into(),
                ty: Ty::SelfType(interface),
                mutable: false,
            }],
            return_type: signature.result.clone(),
        };
        let recursive = BytecodeInstruction::Call {
            dst: None,
            callee: CallTarget::Function(FunctionRef::new(0)),
            args: vec![],
        };
        let program = BytecodeProgram {
            root: ModuleRef::new(1),
            modules: vec![
                BytecodeModule {
                    identity: ModuleIdentity {
                        package: PackageId("fixture".into()),
                        path: vec!["dependency".into()],
                    },
                    functions: vec![BytecodeFunction {
                        name: "foreign".into(),
                        ..Default::default()
                    }],
                    ..Default::default()
                },
                BytecodeModule {
                    identity,
                    trait_contracts: vec![TraitContract {
                        declaration: interface,
                        abi: TraitDef {
                            associated_consts: vec![],
                            name: "Interface".into(),
                            supertraits: vec![],
                            generic_params: vec![],
                            bounds: vec![],
                            methods: vec![declared_method.clone()],
                            associated_types: vec![],
                            conversion_adapter: None,
                        },
                    }],
                    public_items: vec![PublicItem::InterfaceTable(Box::new(InterfaceTable {
                        associated_type_families: vec![],
                        associated_consts: vec![],
                        bridge: None,
                        declaration: table,
                        name: "table".into(),
                        generic_params: vec![],
                        bounds: vec![],
                        trait_type: Ty::Trait(NominalTy {
                            declaration: interface,
                            arguments: vec![t()],
                            associated_types: Default::default(),
                        }),
                        for_type: t(),
                        methods: vec![declared_method.clone()],
                    }))],
                    interface_tables: vec![InterfaceTableRecord {
                        view: None,
                        declaration: table,
                        arguments: vec![t()],
                        methods: vec![],
                        parents: vec![],
                    }],
                    native_declarations: vec![NativeDeclaration {
                        declaration,
                        concrete_result: None,
                        callable_requirements: vec![],
                        function: FnDecl {
                            name: "entry".into(),
                            implementation: CallableImplementation::Native(declaration),
                            params: vec![Param {
                                name: "input".into(),
                                ty: signature.params[0].clone(),
                                mutable: false,
                            }],
                            ..declared_method
                        },
                    }],
                    native_imports: vec![NativeImport {
                        receiver_access: None,
                        result_adapter: None,
                        generic: None,
                        instance: ConcreteFunctionIdentity {
                            declaration,
                            arguments: vec![t()],
                        },
                        binding: declaration,
                        signature,
                        requirements: vec![],
                        callables: vec![],
                    }],
                    functions: vec![BytecodeFunction {
                        identity: Some(ConcreteFunctionIdentity {
                            declaration,
                            arguments: vec![t()],
                        }),
                        name: "not_the_portable_name".into(),
                        instructions: vec![recursive],
                        ..Default::default()
                    }],
                    ..Default::default()
                },
            ],
            exports: Default::default(),
        };
        Self {
            definitions: definitions.freeze(),
            program,
            declaration,
            interface,
            member,
            table,
        }
    }

    fn context(&self) -> DisplayContext<'_> {
        DisplayContext {
            definitions: &self.definitions,
        }
    }

    fn nominal(&self) -> NominalTy {
        NominalTy {
            declaration: self.interface,
            arguments: vec![t()],
            associated_types: Default::default(),
        }
    }

    fn render(&self, instruction: &BytecodeInstruction) -> String {
        let mut output = Vec::new();
        let resolution = Resolution::new(&self.program, &self.definitions);
        write_instruction(&mut output, &self.context(), &resolution, 1, instruction).unwrap();
        let output = String::from_utf8(output).unwrap();
        assert!(!output.ends_with('\n'));
        output
    }
}

fn r(index: usize) -> Slot {
    Slot::new(index)
}

fn t() -> Ty {
    Ty::Builtin(BuiltinType::I32)
}

fn field(reference: Option<ReferenceAccess>) -> FieldRef {
    FieldRef {
        structure: StructId::new(3),
        arguments: vec![t()],
        slot: 7,
        reference,
    }
}

#[test]
fn data_movement_aggregate_path_and_control_flow_operands_are_complete() {
    let fixture = Fixture::new();
    let cases = vec![
        (
            BytecodeInstruction::BinaryOperands {
                dst: r(0),
                op: BinaryOp::Sub,
                lhs: ScalarOperand::Constant {
                    constant: ConstantId::new(9),
                    ty: BuiltinType::I16,
                },
                rhs: ScalarOperand::Slot(r(1)),
            },
            "binary_operands.sub dst=s0, lhs=@1:const#9:i16, rhs=s1",
        ),
        (
            BytecodeInstruction::PredicateBranch {
                op: BinaryOp::Lt,
                lhs: ScalarOperand::Slot(r(1)),
                rhs: ScalarOperand::Constant {
                    constant: ConstantId::new(9),
                    ty: BuiltinType::F64,
                },
                negated: true,
                then_target: JumpTarget::new(2),
                else_target: JumpTarget::new(3),
            },
            "predicate_branch.lt negated=true, lhs=s1, rhs=@1:const#9:f64, then=L2, else=L3",
        ),
        (
            BytecodeInstruction::Await {
                dst: r(0),
                value: r(1),
                future: t(),
            },
            "await s0, value=s1, future=i32",
        ),
        (
            BytecodeInstruction::LoadConst {
                dst: r(0),
                constant: ConstantId::new(9),
            },
            "load_const s0, @1:const#9",
        ),
        (
            BytecodeInstruction::Move {
                dst: r(0),
                src: Slot::new(8),
            },
            "move s0, s8",
        ),
        (
            BytecodeInstruction::LoadModule {
                dst: r(0),
                slot: ModuleSlot::new(8),
            },
            "load_module s0, @1:slot#8",
        ),
        (
            BytecodeInstruction::Move {
                dst: Slot::new(8),
                src: r(1),
            },
            "move s8, s1",
        ),
        (
            BytecodeInstruction::StoreModule {
                slot: ModuleSlot::new(8),
                src: r(1),
            },
            "store_module @1:slot#8, s1",
        ),
        (
            BytecodeInstruction::Move {
                dst: r(0),
                src: r(1),
            },
            "move s0, s1",
        ),
        (
            BytecodeInstruction::MakeTuple {
                dst: r(0),
                elements: vec![r(2), r(1)],
            },
            "make_tuple s0, elements=[s2, s1]",
        ),
        (
            BytecodeInstruction::MakeTuple {
                dst: r(0),
                elements: vec![],
            },
            "make_tuple s0, elements=[]",
        ),
        (
            BytecodeInstruction::RangeBound {
                dst: r(0),
                value: r(1),
                range: t(),
                bound: Ty::Builtin(BuiltinType::U64),
                upper: true,
            },
            "range_bound s0, value=s1, range=i32, bound=u64, upper=true",
        ),
        (
            BytecodeInstruction::RangeBound {
                dst: r(0),
                value: r(1),
                range: t(),
                bound: t(),
                upper: false,
            },
            "range_bound s0, value=s1, range=i32, bound=i32, upper=false",
        ),
        (
            BytecodeInstruction::MakeRange {
                dst: r(0),
                start: Some(r(1)),
                end: None,
                ty: t(),
            },
            "make_range s0, start=s1, end=none, type=i32",
        ),
        (
            BytecodeInstruction::MakeRange {
                dst: r(0),
                start: None,
                end: Some(r(2)),
                ty: t(),
            },
            "make_range s0, start=none, end=s2, type=i32",
        ),
        (
            BytecodeInstruction::RepeatArray {
                element: t(),
                dst: r(0),
                value: r(1),
                count: r(2),
            },
            "repeat_array s0, value=s1, count=s2, element=i32",
        ),
        (
            BytecodeInstruction::MakeArray {
                element: t(),
                dst: r(0),
                elements: vec![r(2), r(1)],
            },
            "make_array s0, element=i32, elements=[s2, s1]",
        ),
        (
            BytecodeInstruction::MakeFuture {
                dst: r(0),
                function: FunctionRef::new(0),
                arguments: vec![r(2), r(1)],
                future: t(),
            },
            "make_future s0, function=@1:0 fixture::app::entry arguments=[i32], arguments=[s2, s1], future=i32",
        ),
        (
            BytecodeInstruction::MakeClosure {
                dst: r(0),
                function: FunctionRef::new(0),
                captures: vec![r(2), r(1)],
            },
            "make_closure s0, function=@1:0 fixture::app::entry arguments=[i32], captures=[s2, s1]",
        ),
        (
            BytecodeInstruction::MakeCell {
                dst: r(0),
                value: r(1),
            },
            "make_cell s0, value=s1",
        ),
        (
            BytecodeInstruction::ReadCell {
                dst: r(0),
                cell: r(1),
            },
            "read_cell s0, cell=s1",
        ),
        (
            BytecodeInstruction::WriteCell {
                cell: r(1),
                value: r(2),
            },
            "write_cell cell=s1, value=s2",
        ),
        (
            BytecodeInstruction::UpcastInterface {
                dst: r(0),
                value: r(1),
                source: fixture.nominal(),
                target: NominalTy {
                    declaration: fixture.interface,
                    arguments: vec![],
                    associated_types: Default::default(),
                },
            },
            "upcast_interface s0, value=s1, source=trait fixture::app::Interface<i32>, target=trait fixture::app::Interface",
        ),
        (
            BytecodeInstruction::MakeInterface {
                dst: r(0),
                value: r(1),
                target: BytecodeConstruction::Declared {
                    module: ModuleRef::new(0),
                    implementation: InterfaceTableRef::new(5),
                    arguments: vec![t()],
                },
            },
            "make_interface s0, value=s1, @0:interface#5, arguments=[i32]",
        ),
        (
            BytecodeInstruction::MakeStruct {
                dst: r(0),
                structure: StructId::new(3),
                arguments: vec![t()],
                fields: vec![r(2), r(1)],
            },
            "make_struct s0, @1:struct#3, arguments=[i32], fields=[s2, s1]",
        ),
        (
            BytecodeInstruction::MakeEnum {
                dst: r(0),
                enumeration: EnumId::new(4),
                arguments: vec![t()],
                variant: 6,
                fields: vec![r(2), r(1)],
            },
            "make_enum s0, @1:enum#4, arguments=[i32], variant=6, fields=[s2, s1]",
        ),
        (
            BytecodeInstruction::TestEnumVariant {
                dst: r(0),
                value: r(1),
                enumeration: EnumId::new(4),
                arguments: vec![t()],
                variant: 6,
            },
            "test_enum_variant s0, value=s1, @1:enum#4, arguments=[i32], variant=6",
        ),
        (
            BytecodeInstruction::ReadEnumPayload {
                dst: r(0),
                value: r(1),
                enumeration: EnumId::new(4),
                arguments: vec![t()],
                variant: 6,
                index: 7,
            },
            "read_enum_payload s0, value=s1, @1:enum#4, arguments=[i32], variant=6, index=7",
        ),
        (
            BytecodeInstruction::ReadAggregateField {
                dst: r(0),
                base: r(1),
                field: field(None),
            },
            "read_aggregate_field s0, base=s1, field={@1:struct#3, arguments=[i32], slot=7, reference=none}",
        ),
        (
            BytecodeInstruction::WriteAggregateField {
                base: r(1),
                field: field(Some(ReferenceAccess::Mutable)),
                value: r(2),
            },
            "write_aggregate_field base=s1, field={@1:struct#3, arguments=[i32], slot=7, reference=mutable}, value=s2",
        ),
        (
            BytecodeInstruction::ReadAggregateField {
                dst: r(0),
                base: r(1),
                field: field(Some(ReferenceAccess::Shared)),
            },
            "read_aggregate_field s0, base=s1, field={@1:struct#3, arguments=[i32], slot=7, reference=shared}",
        ),
        (
            BytecodeInstruction::ReadAggregateIndex {
                dst: r(0),
                base: r(1),
                index: r(2),
            },
            "read_aggregate_index s0, base=s1, index=s2",
        ),
        (
            BytecodeInstruction::WriteAggregateIndex {
                base: r(1),
                index: r(2),
                value: r(3),
            },
            "write_aggregate_index base=s1, index=s2, value=s3",
        ),
        (
            BytecodeInstruction::ReadPath {
                dst: r(0),
                root_or_view: r(1),
                path: PathId::new(6),
                dynamic_args: vec![r(3), r(2)],
            },
            "read_path s0, root_or_view=s1, @1:path#6, dynamic_args=[s3, s2]",
        ),
        (
            BytecodeInstruction::SetPath {
                root_or_view: r(1),
                path: PathId::new(6),
                dynamic_args: vec![r(3), r(2)],
                value: r(4),
            },
            "set_path root_or_view=s1, @1:path#6, dynamic_args=[s3, s2], value=s4",
        ),
        (
            BytecodeInstruction::ModifyPath {
                dst: Some(r(0)),
                root_or_view: r(1),
                path: PathId::new(6),
                dynamic_args: vec![r(3), r(2)],
                op: BinaryOp::IdentityEq,
                value: r(4),
            },
            "modify_path dst=s0, root_or_view=s1, @1:path#6, dynamic_args=[s3, s2], op=identity_eq, value=s4",
        ),
        (
            BytecodeInstruction::ModifyPath {
                dst: None,
                root_or_view: r(1),
                path: PathId::new(6),
                dynamic_args: vec![],
                op: BinaryOp::Add,
                value: r(4),
            },
            "modify_path dst=none, root_or_view=s1, @1:path#6, dynamic_args=[], op=add, value=s4",
        ),
        (
            BytecodeInstruction::MakePathView {
                dst: r(0),
                root_or_view: r(1),
                path: PathId::new(6),
                dynamic_args: vec![r(3), r(2)],
            },
            "make_path_view s0, root_or_view=s1, @1:path#6, dynamic_args=[s3, s2]",
        ),
        (
            BytecodeInstruction::Jump {
                target: JumpTarget::new(12345),
            },
            "jump L12345",
        ),
        (
            BytecodeInstruction::Branch {
                cond: r(1),
                then_target: JumpTarget::new(7),
                else_target: JumpTarget::new(2),
            },
            "branch s1, then=L7, else=L2",
        ),
        (BytecodeInstruction::Return(Some(r(1))), "return s1"),
        (BytecodeInstruction::Return(None), "return none"),
        (BytecodeInstruction::Unreachable, "unreachable"),
    ];
    for (instruction, expected) in cases {
        assert_eq!(fixture.render(&instruction), expected);
    }
}

struct FailAfter {
    remaining: usize,
    failed: bool,
}

impl Write for FailAfter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        assert!(!self.failed, "renderer wrote again after an IO error");
        if self.remaining == 0 {
            self.failed = true;
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "test sink closed",
            ));
        }
        let written = bytes.len().min(self.remaining);
        self.remaining -= written;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        panic!("renderer must not flush the caller's writer")
    }
}

fn assert_streaming_failures(fixture: &Fixture, instruction: &BytecodeInstruction) {
    let expected = fixture.render(instruction);
    let resolution = Resolution::new(&fixture.program, &fixture.definitions);
    for remaining in 0..expected.len() {
        let mut writer = FailAfter {
            remaining,
            failed: false,
        };
        let error = write_instruction(&mut writer, &fixture.context(), &resolution, 1, instruction)
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
        assert_eq!(error.to_string(), "test sink closed");
        assert!(writer.failed);
    }
}

#[test]
fn output_is_append_only_and_stops_at_each_possible_write_failure() {
    let fixture = Fixture::new();
    let instruction = BytecodeInstruction::MakeStruct {
        dst: r(0),
        structure: StructId::new(3),
        arguments: vec![t(), Ty::Array(Box::new(t()))],
        fields: vec![r(2), r(1)],
    };
    assert_streaming_failures(&fixture, &instruction);
    let mut output = b"prefix ".to_vec();
    write_instruction(
        &mut output,
        &fixture.context(),
        &Resolution::new(&fixture.program, &fixture.definitions),
        1,
        &instruction,
    )
    .unwrap();
    assert_eq!(
        String::from_utf8(output).unwrap(),
        format!("prefix {}", fixture.render(&instruction))
    );
}

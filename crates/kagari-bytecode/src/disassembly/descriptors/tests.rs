//! Pure presentation tests; production entrypoints supply verified programs only.

use crate::{
    disassembly::{
        descriptors::{Category, References},
        presentation::DisplayContext,
        resolution::Resolution,
    },
    instruction::{
        BytecodeInstruction as I, CallTarget, ConstantId, ConstantOperand,
        ConstructionTarget as BytecodeConstruction, EnumId, FieldRef, InterfaceTableRef,
        ModuleSlot, NativeImportId, PathId, Slot, StructId,
    },
    module::{
        BytecodeFunction, BytecodeModule, CallableTarget, InterfaceMethodSlot,
        InterfaceParentRecord, InterfaceResultAdapter, InterfaceTableRecord, InterfaceViewRecord,
        PathRecord,
    },
    program::{BytecodeProgram, ModuleRef},
};
use kagari_abi::representation::ValueType;
use kagari_common::identity::{
    DefinitionKind, ModuleIdentity, PackageId,
    table::{DefinitionId, DefinitionTable, DefinitionTableBuilder},
};
use kagari_contract::{
    callable::{shared::SharedCall, witness::OperationWitness},
    effects::EffectSet,
    ids::FunctionRef,
    layout::{EnumLayout, EnumVariantLayout, StructFieldLayout, StructLayout},
    native_import::{
        NativeImport,
        callables::{NativeCallableApplication, NativeCallableOrigin},
        result::NativeResultAdapter,
    },
    statics::{StaticContract, StaticDescriptor},
    types::{ConcreteFunctionIdentity, InterfaceTable, PublicItem},
};
use kagari_types::{
    callable::{CallableImplementation, MethodPolicy, Signature},
    declaration::{
        FieldDef, FnDecl, NativeDeclaration, Param, TypeDeclaration, TypeDef, TypeDefKind,
        host::{
            HostCallPolicy, HostFieldPolicy, HostFunctionEffects, HostPassingStyle,
            HostReflectionPolicy, HostTypeOwnership, HostTypePolicy, PathAccess,
        },
        path::{
            HostIndexSegmentDeclaration, HostPathDeclaration, HostPathSegmentDeclaration,
            HostVirtualSegmentDeclaration,
        },
        requirement::NativeCallableRequirement,
    },
    reference::ReferenceAccess,
    scalar::BuiltinType,
    ty::{GenericBound, GenericParam, NominalTy, Ty},
    visibility::Visibility,
};
use std::io::{self, Write};

fn module(name: &str) -> BytecodeModule {
    BytecodeModule {
        identity: ModuleIdentity {
            package: PackageId(name.into()),
            path: vec![],
        },
        ..Default::default()
    }
}

fn program(modules: Vec<BytecodeModule>) -> BytecodeProgram {
    BytecodeProgram {
        root: ModuleRef::new(0),
        modules,
        exports: Default::default(),
    }
}

fn body(instructions: Vec<I>) -> BytecodeFunction {
    BytecodeFunction {
        instructions,
        ..Default::default()
    }
}

fn child(
    builder: &mut DefinitionTableBuilder,
    owner: DefinitionId,
    kind: DefinitionKind,
    name: &str,
) -> DefinitionId {
    builder.intern_child(owner, kind, name, 0).unwrap()
}

fn identity(declaration: DefinitionId) -> ConcreteFunctionIdentity {
    ConcreteFunctionIdentity {
        declaration,
        arguments: vec![],
    }
}

fn nominal(declaration: DefinitionId) -> NominalTy {
    NominalTy {
        declaration,
        arguments: vec![],
        associated_types: Default::default(),
    }
}

fn integer() -> Ty {
    Ty::Builtin(BuiltinType::I32)
}

fn render(
    program: &BytecodeProgram,
    selected: &[(usize, usize)],
    definitions: &DefinitionTable,
) -> String {
    let mut output = Vec::new();
    let resolution = Resolution::new(program, definitions);
    References::collect(&resolution, selected)
        .write(&mut output, &DisplayContext { definitions }, &resolution)
        .unwrap();
    String::from_utf8(output).unwrap()
}

#[test]
fn constants_preserve_bits_and_escape_text_once_in_slot_order() {
    let mut owner = module("values");
    owner.constants = vec![
        ConstantOperand::Unit,
        ConstantOperand::Bool(true),
        ConstantOperand::I32(i32::MIN),
        ConstantOperand::I64(i64::MIN),
        ConstantOperand::U64(u64::MAX),
        ConstantOperand::F32(-0.0),
        ConstantOperand::F32(f32::from_bits(0x7fc0_0042)),
        ConstantOperand::F64(-0.0),
        ConstantOperand::F64(f64::from_bits(0x7ff8_0000_0000_0042)),
        ConstantOperand::Str("hello\n\"\\\u{1b}\u{202e}雪".into()),
        ConstantOperand::Str("unreferenced".into()),
    ];
    let instructions = (0..10)
        .rev()
        .chain([9, 0])
        .map(|slot| I::LoadConst {
            dst: Slot::new(0),
            constant: ConstantId::new(slot),
        })
        .collect();
    owner.functions.push(body(instructions));
    let definitions = DefinitionTableBuilder::new().unwrap().freeze();
    let output = render(&program(vec![owner]), &[(0, 0), (0, 0)], &definitions);
    assert_eq!(
        output,
        concat!(
            "descriptors @0: values\n",
            "  @0:const#0 unit ()\n",
            "  @0:const#1 bool true\n",
            "  @0:const#2 i32 -2147483648\n",
            "  @0:const#3 i64 -9223372036854775808\n",
            "  @0:const#4 u64 18446744073709551615\n",
            "  @0:const#5 f32 -0 bits=0x80000000\n",
            "  @0:const#6 f32 NaN bits=0x7fc00042\n",
            "  @0:const#7 f64 -0 bits=0x8000000000000000\n",
            "  @0:const#8 f64 NaN bits=0x7ff8000000000042\n",
            "  @0:const#9 str \"hello\\n\\\"\\\\\\u{1b}\\u{202e}雪\"\n",
        )
    );
}

#[test]
fn layouts_and_module_slots_have_access_types_and_nested_dependencies() {
    let mut owner = module("layouts");
    let mut builder = DefinitionTableBuilder::new().unwrap();
    let root = builder.intern_root(&owner.identity).unwrap();
    let structure = child(&mut builder, root, DefinitionKind::Struct, "Box");
    let enumeration = child(&mut builder, root, DefinitionKind::Enum, "State");
    let field = child(&mut builder, structure, DefinitionKind::Field, "state\n");
    let variant = child(
        &mut builder,
        enumeration,
        DefinitionKind::Variant,
        "Failure",
    );
    owner.structures.push(StructLayout {
        declaration: structure,
        arguments: vec![],
        fields: vec![StructFieldLayout {
            declaration: field,
            name: "state\n".into(),
            ty: Ty::Enum(nominal(enumeration)),
            visibility: Visibility::PublicSuper,
            mutable: true,
        }],
    });
    // The payload cycles back to Box: collection must terminate without copying it.
    owner.enumerations.push(EnumLayout {
        declaration: enumeration,
        arguments: vec![],
        variants: vec![EnumVariantLayout {
            declaration: variant,
            reports_failure: true,
            payload: vec![Ty::Struct(nominal(structure))],
        }],
    });
    owner.module_slots.push(StaticDescriptor {
        contract: StaticContract {
            declaration: child(&mut builder, root, DefinitionKind::Static, "counter\n"),
            supplier: owner.identity.clone(),
            name: "counter\n".into(),
            ty: Ty::Builtin(BuiltinType::I64),
            mutable: false,
            visibility: Visibility::Private,
        },
        initializer: FunctionRef::new(0),
    });
    owner.functions.push(body(vec![
        I::StoreModule {
            slot: ModuleSlot::new(0),
            src: Slot::new(0),
        },
        I::ReadAggregateField {
            dst: Slot::new(0),
            base: Slot::new(1),
            field: FieldRef {
                structure: StructId::new(0),
                arguments: vec![],
                slot: 0,
                reference: None,
            },
        },
    ]));
    let output = render(&program(vec![owner]), &[(0, 0)], &builder.freeze());
    assert!(output.contains("field#0 layouts::Box::\"state\\n\" name=\"state\\n\" type=enum layouts::State visibility=pub(super) mutable=true"));
    assert!(output.contains("variant#0 layouts::State::Failure reports_failure=true"));
    assert!(output.contains("payload#0 type=struct layouts::Box"));
    assert!(output.contains("@0:slot#0 name=\"counter\\n\" physical=i64 mutable=false"));
    assert_eq!(output.matches("  @0:struct#0").count(), 1);
    assert_eq!(output.matches("  @0:enum#0").count(), 1);
    assert!(output.find("  @0:struct#0").unwrap() < output.find("  @0:enum#0").unwrap());
    assert!(output.find("  @0:enum#0").unwrap() < output.find("  @0:slot#0").unwrap());
}

#[test]
fn interface_closure_crosses_modules_but_does_not_follow_callee_bodies() {
    let mut caller = module("caller");
    let mut owner = module("owner");
    let mut builder = DefinitionTableBuilder::new().unwrap();
    let root = builder.intern_root(&owner.identity).unwrap();
    let first = child(&mut builder, root, DefinitionKind::Function, "first_table");
    let second = child(&mut builder, root, DefinitionKind::Function, "second_table");
    let trait_id = child(&mut builder, root, DefinitionKind::Trait, "Reader");
    let method = child(&mut builder, trait_id, DefinitionKind::Method, "read");
    owner.public_items.extend([
        interface_declaration(first, trait_id),
        interface_declaration(second, trait_id),
    ]);
    let parent = |target| InterfaceParentRecord {
        interface: nominal(trait_id),
        implementation: identity(target),
        view: false,
    };
    owner.interface_tables = vec![
        InterfaceTableRecord {
            declaration: first,
            arguments: vec![],
            methods: vec![InterfaceMethodSlot {
                method,
                target: CallableTarget::Script(FunctionRef::new(0)),
                arguments: vec![integer()],
            }],
            parents: vec![parent(second)],
            view: Some(InterfaceViewRecord {
                interface: nominal(trait_id),
                results: vec![InterfaceResultAdapter {
                    method,
                    implementation: identity(second),
                }],
            }),
        },
        InterfaceTableRecord {
            declaration: second,
            arguments: vec![],
            methods: vec![],
            parents: vec![parent(first)],
            view: None,
        },
    ];
    owner
        .constants
        .push(ConstantOperand::Str("callee-only literal".into()));
    owner.functions.push(body(vec![I::LoadConst {
        dst: Slot::new(0),
        constant: ConstantId::new(0),
    }]));
    caller.functions.push(body(vec![I::MakeInterface {
        dst: Slot::new(0),
        value: Slot::new(1),
        target: BytecodeConstruction::Declared {
            module: ModuleRef::new(1),
            implementation: InterfaceTableRef::new(0),
            arguments: vec![],
        },
    }]));
    let program = program(vec![caller, owner]);
    let definitions = builder.freeze();
    let resolution = Resolution::new(&program, &definitions);
    let references = References::collect(&resolution, &[(0, 0)]);
    assert!(
        references
            .entries
            .iter()
            .copied()
            .eq([(1, Category::Interface, 0), (1, Category::Interface, 1)])
    );
    let output = render(&program, &[(0, 0)], &definitions);
    assert!(output.contains("member#0 owner::Reader::read target=@1:0 arguments=[i32]"));
    assert!(output.contains("template trait_type=trait owner::Reader for_type=i32"));
    assert!(output.contains("parent#0 @1:interface#1"));
    assert!(output.contains("result_adapter#0 method=owner::Reader::read target=@1:interface#1"));
    assert_eq!(output.matches("  @1:interface#0").count(), 1);
    assert_eq!(output.matches("  @1:interface#1").count(), 1);
    assert!(!output.contains("callee-only"));
    assert!(!output.contains("descriptors @0"));
}

fn interface_declaration(declaration: DefinitionId, interface: DefinitionId) -> PublicItem {
    PublicItem::InterfaceTable(Box::new(InterfaceTable {
        associated_type_families: vec![],
        associated_consts: vec![],
        bridge: None,
        declaration,
        name: "table".into(),
        generic_params: vec![],
        bounds: vec![],
        trait_type: Ty::Trait(nominal(interface)),
        for_type: integer(),
        methods: vec![],
    }))
}

fn native_declaration(declaration: DefinitionId, binding: DefinitionId) -> NativeDeclaration {
    NativeDeclaration {
        declaration,
        concrete_result: Some(integer()),
        callable_requirements: vec![],
        function: FnDecl {
            name: "entry".into(),
            implementation: CallableImplementation::Native(binding),
            method_policy: MethodPolicy {
                override_allowed: false,
                receiver_access: Some(ReferenceAccess::Shared),
            },
            generic_params: vec![],
            bounds: vec![],
            params: vec![Param {
                name: "input\n".into(),
                ty: integer(),
                mutable: false,
            }],
            return_type: integer(),
            host_policy: Some(HostCallPolicy {
                passing: vec![HostPassingStyle::Owned],
                effects: HostFunctionEffects {
                    may_trap: true,
                    may_read_immutable_configuration: true,
                    ..Default::default()
                },
            }),
        },
    }
}

fn native_import(declaration: DefinitionId, binding: DefinitionId) -> NativeImport {
    NativeImport {
        receiver_access: Some(ReferenceAccess::Shared),
        result_adapter: None,
        generic: None,
        instance: identity(declaration),
        binding,
        signature: Signature {
            params: vec![integer()],
            result: integer(),
        },
        requirements: vec![],
        callables: vec![],
    }
}

#[test]
fn native_dependencies_use_declaration_owner_and_print_offline_contracts() {
    let mut caller = module("caller");
    let mut owner = module("host");
    let mut builder = DefinitionTableBuilder::new().unwrap();
    let root = builder.intern_root(&owner.identity).unwrap();
    let declaration = child(&mut builder, root, DefinitionKind::Function, "entry");
    let binding = child(&mut builder, root, DefinitionKind::Function, "rust_entry");
    let interface = child(&mut builder, root, DefinitionKind::Trait, "Read");
    let method = child(&mut builder, interface, DefinitionKind::Method, "read");
    let table = child(&mut builder, root, DefinitionKind::Function, "read_table");
    let required = NativeCallableRequirement {
        receiver: integer(),
        interface: nominal(interface),
        member: method,
        arguments: vec![],
    };
    let selected = OperationWitness::Selected(Box::new(NativeCallableApplication {
        origin: NativeCallableOrigin::Implementation,
        requirement: required.clone(),
        instance: identity(declaration),
        implementation: CallableImplementation::Native(binding),
        signature: Signature {
            params: vec![integer()],
            result: integer(),
        },
        effects: EffectSet::native_call(),
    }));
    let mut import = native_import(declaration, binding);
    import.result_adapter = Some(NativeResultAdapter {
        receiver: integer(),
        implementation: identity(table),
    });
    import.requirements.push(GenericBound {
        ty: integer(),
        constraints: vec![nominal(interface)],
    });
    import.callables = vec![
        selected.clone(),
        OperationWitness::Forward(Box::new(required.clone())),
    ];
    // An earlier consumer copy must not steal a witness's owner-native slot.
    caller.native_imports.push(import.clone());
    owner.native_imports.push(import);
    let mut declaration_record = native_declaration(declaration, binding);
    declaration_record.callable_requirements.push(required);
    owner.native_declarations.push(declaration_record);
    owner
        .public_items
        .push(interface_declaration(table, interface));
    owner.interface_tables.push(InterfaceTableRecord {
        declaration: table,
        arguments: vec![],
        methods: vec![InterfaceMethodSlot {
            method,
            target: CallableTarget::Native(NativeImportId::new(0)),
            arguments: vec![],
        }],
        parents: vec![],
        view: None,
    });
    caller.functions.push(body(vec![I::Call {
        dst: None,
        callee: CallTarget::Shared {
            module: ModuleRef::new(1),
            target: CallableTarget::Script(FunctionRef::new(0)),
            contract: Box::new(SharedCall {
                constructions: vec![],
                receiver_access: None,
                instance: identity(declaration),
                implementation: CallableImplementation::Script,
                arguments: vec![integer()],
                signature: Signature {
                    params: vec![],
                    result: integer(),
                },
                operations: vec![selected],
            }),
        },
        args: vec![],
    }]));
    let program = program(vec![caller, owner]);
    let output = render(&program, &[(0, 0)], &builder.freeze());
    assert!(!output.contains("@0:native"));
    assert_eq!(output.matches("  @1:native#0").count(), 1);
    assert!(
        output.contains("binding=host::rust_entry receiver_access=shared installation=not_checked")
    );
    assert!(output.contains("signature params=[i32] -> i32"));
    assert!(output.contains("operation#0 selected{target=@1:native#0, origin=implementation"));
    assert!(output.contains("operation#1 forward{receiver=i32"));
    assert!(output.contains("result_adapter receiver=i32 target=@1:interface#0"));
    assert!(output.contains("constraint#0 host::Read"));
    assert!(output.contains("declaration host::entry"));
    assert!(output.contains("param#0 name=\"input\\n\" type=i32 mutable=false"));
    assert!(output.contains("callable_requirement#0 receiver=i32"));
    assert!(output.contains("host_passing#0 owned"));
    assert!(output.contains("may_read_immutable_configuration=true"));
}

#[test]
fn paths_show_retained_facts_without_rebuilding_declarations() {
    let mut owner = module("host");
    let mut builder = DefinitionTableBuilder::new().unwrap();
    let root = builder.intern_root(&owner.identity).unwrap();
    let player = child(&mut builder, root, DefinitionKind::Struct, "Player");
    let field = child(&mut builder, player, DefinitionKind::Field, "values");
    let array = Ty::Array(Box::new(integer()));
    owner.external_types.push(TypeDeclaration {
        declaration: player,
        definition: TypeDef {
            name: "Player".into(),
            kind: TypeDefKind::Host(HostTypePolicy {
                ownership: HostTypeOwnership::HostRoot,
                reflection: HostReflectionPolicy::Metadata,
                path_access: PathAccess::ReadWrite,
            }),
            generic_params: vec![],
            bounds: vec![],
            fields: vec![FieldDef {
                host_policy: Some(HostFieldPolicy {
                    readable: true,
                    path_access: PathAccess::ReadWrite,
                }),
                visibility: Visibility::Public,
                name: "values".into(),
                ty: array.clone(),
                mutable: true,
            }],
            variants: vec![],
        },
    });
    let path = HostPathDeclaration {
        root: player,
        schema_epoch: 42,
        access: PathAccess::ReadOnly,
        segments: vec![
            HostPathSegmentDeclaration::Field(field),
            HostPathSegmentDeclaration::Index(HostIndexSegmentDeclaration {
                slot: 2,
                collection: array,
                index: integer(),
                result: integer(),
                access: PathAccess::ReadOnly,
            }),
            HostPathSegmentDeclaration::Virtual(HostVirtualSegmentDeclaration {
                name: "adjust\n".into(),
                result: integer(),
                access: PathAccess::ReadOnly,
            }),
        ],
    };
    let definitions = builder.freeze();
    // Pure presentation fixture: even unrelated unadmitted declarations must not
    // be rebuilt by the renderer. Production admission remains with the verifier.
    let fingerprint = 0x1234;
    let mut unused = path.clone();
    unused.schema_epoch = 99;
    owner.path_declarations = vec![unused, path];
    owner.paths.push(PathRecord {
        contract_fingerprint: fingerprint,
        id: PathId::new(0),
        root_ty: ValueType::HostHandle,
        result_ty: ValueType::I32,
        read_only: true,
        debug_name: "player\nvalues".into(),
    });
    owner.functions.push(body(vec![I::ReadPath {
        dst: Slot::new(0),
        root_or_view: Slot::new(1),
        path: PathId::new(0),
        dynamic_args: vec![],
    }]));
    let output = render(&program(vec![owner]), &[(0, 0)], &definitions);
    assert!(output.contains(&format!("@0:path#0 id=0 fingerprint=0x{fingerprint:016x} root_physical=host_handle result_physical=i32 read_only=true")));
    assert!(output.contains("name=\"player\\nvalues\""));
    assert_eq!(output.matches("@0:path#0").count(), 1);
    assert!(!output.contains("schema_epoch="));
}

#[test]
fn every_direct_table_operand_is_collected_without_rendering_unselected_bodies() {
    let r = Slot::new(0);
    let field = FieldRef {
        structure: StructId::new(0),
        arguments: vec![],
        slot: 0,
        reference: None,
    };
    let mut owner = module("operands");
    // Only the referenced categories whose closure is inspected need records.
    let mut builder = DefinitionTableBuilder::new().unwrap();
    let root = builder.intern_root(&owner.identity).unwrap();
    let structure = child(&mut builder, root, DefinitionKind::Struct, "S");
    let enumeration = child(&mut builder, root, DefinitionKind::Enum, "E");
    let native = child(&mut builder, root, DefinitionKind::Function, "native");
    owner.structures.push(StructLayout {
        declaration: structure,
        arguments: vec![],
        fields: vec![],
    });
    owner.enumerations.push(EnumLayout {
        declaration: enumeration,
        arguments: vec![],
        variants: vec![],
    });
    owner.native_imports.push(native_import(native, native));
    let cases = vec![
        (
            Category::Constant,
            I::LoadConst {
                dst: r,
                constant: ConstantId::new(0),
            },
        ),
        (
            Category::Slot,
            I::LoadModule {
                dst: r,
                slot: ModuleSlot::new(0),
            },
        ),
        (
            Category::Slot,
            I::StoreModule {
                slot: ModuleSlot::new(0),
                src: r,
            },
        ),
        (
            Category::Struct,
            I::MakeStruct {
                dst: r,
                structure: StructId::new(0),
                arguments: vec![],
                fields: vec![],
            },
        ),
        (
            Category::Struct,
            I::ReadAggregateField {
                dst: r,
                base: r,
                field: field.clone(),
            },
        ),
        (
            Category::Struct,
            I::WriteAggregateField {
                base: r,
                field,
                value: r,
            },
        ),
        (
            Category::Enum,
            I::MakeEnum {
                dst: r,
                enumeration: EnumId::new(0),
                arguments: vec![],
                variant: 0,
                fields: vec![],
            },
        ),
        (
            Category::Enum,
            I::TestEnumVariant {
                dst: r,
                value: r,
                enumeration: EnumId::new(0),
                arguments: vec![],
                variant: 0,
            },
        ),
        (
            Category::Enum,
            I::ReadEnumPayload {
                dst: r,
                value: r,
                enumeration: EnumId::new(0),
                arguments: vec![],
                variant: 0,
                index: 0,
            },
        ),
        (
            Category::Path,
            I::ReadPath {
                dst: r,
                root_or_view: r,
                path: PathId::new(0),
                dynamic_args: vec![],
            },
        ),
        (
            Category::Path,
            I::SetPath {
                root_or_view: r,
                path: PathId::new(0),
                dynamic_args: vec![],
                value: r,
            },
        ),
        (
            Category::Path,
            I::ModifyPath {
                dst: None,
                root_or_view: r,
                path: PathId::new(0),
                dynamic_args: vec![],
                op: crate::instruction::BinaryOp::Add,
                value: r,
            },
        ),
        (
            Category::Path,
            I::MakePathView {
                dst: r,
                root_or_view: r,
                path: PathId::new(0),
                dynamic_args: vec![],
            },
        ),
        (
            Category::Native,
            I::Call {
                dst: None,
                callee: CallTarget::Native(NativeImportId::new(0)),
                args: vec![],
            },
        ),
    ];
    owner.functions.push(body(vec![]));
    let mut program = program(vec![owner]);
    let definitions = builder.freeze();
    for (category, instruction) in cases {
        program.modules[0].functions[0].instructions = vec![instruction];
        let resolution = Resolution::new(&program, &definitions);
        let references = References::collect(&resolution, &[(0, 0)]);
        assert!(references.entries.iter().copied().eq([(0, category, 0)]));
    }
}

#[test]
fn symbolic_result_adapters_select_the_canonical_generic_table() {
    let mut owner = module("generic");
    let mut builder = DefinitionTableBuilder::new().unwrap();
    let root = builder.intern_root(&owner.identity).unwrap();
    let declaration = child(&mut builder, root, DefinitionKind::Function, "entry");
    let table_id = child(&mut builder, root, DefinitionKind::Impl, "table");
    let trait_id = child(&mut builder, root, DefinitionKind::Trait, "Read");
    let parameter = GenericParam {
        owner: table_id,
        position: 0,
    };
    let symbolic = Ty::Parameter {
        owner: declaration,
        position: 0,
    };
    let mut import = native_import(declaration, declaration);
    import.result_adapter = Some(NativeResultAdapter {
        receiver: symbolic.clone(),
        implementation: ConcreteFunctionIdentity {
            declaration: table_id,
            arguments: vec![symbolic],
        },
    });
    owner.native_imports.push(import);
    owner
        .native_declarations
        .push(native_declaration(declaration, declaration));
    owner
        .public_items
        .push(PublicItem::InterfaceTable(Box::new(InterfaceTable {
            associated_type_families: vec![],
            associated_consts: vec![],
            bridge: None,
            declaration: table_id,
            name: "table".into(),
            generic_params: vec![parameter.clone()],
            bounds: vec![],
            trait_type: Ty::Trait(nominal(trait_id)),
            for_type: parameter.as_type(),
            methods: vec![],
        })));
    for argument in [integer(), parameter.as_type()] {
        owner.interface_tables.push(InterfaceTableRecord {
            view: None,
            arguments: vec![argument],
            declaration: table_id,
            methods: vec![],
            parents: vec![],
        });
    }
    owner.functions.push(body(vec![I::Call {
        dst: None,
        callee: CallTarget::Native(NativeImportId::new(0)),
        args: vec![],
    }]));
    let program = program(vec![owner]);
    let definitions = builder.freeze();
    let resolution = Resolution::new(&program, &definitions);
    let references = References::collect(&resolution, &[(0, 0)]);
    assert!(
        references
            .entries
            .iter()
            .copied()
            .eq([(0, Category::Interface, 1), (0, Category::Native, 0)])
    );
    let output = render(&program, &[(0, 0)], &definitions);
    assert!(output.contains("target=@0:interface#1"));
    assert!(!output.contains("@0:interface#0"));
}

#[test]
fn writer_errors_stop_immediately_and_empty_selection_writes_nothing() {
    struct Closed {
        calls: usize,
    }

    impl Write for Closed {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            self.calls += 1;
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed"))
        }

        fn flush(&mut self) -> io::Result<()> {
            panic!("descriptor writer must not flush")
        }
    }

    let mut owner = module("io");
    owner.constants.push(ConstantOperand::Unit);
    owner.functions.push(body(vec![I::LoadConst {
        dst: Slot::new(0),
        constant: ConstantId::new(0),
    }]));
    let program = program(vec![owner]);
    let definitions = DefinitionTableBuilder::new().unwrap().freeze();
    let context = DisplayContext {
        definitions: &definitions,
    };
    let mut writer = Closed { calls: 0 };
    let resolution = Resolution::new(&program, &definitions);
    References::collect(&resolution, &[])
        .write(&mut writer, &context, &resolution)
        .unwrap();
    assert_eq!(writer.calls, 0);
    let error = References::collect(&resolution, &[(0, 0)])
        .write(&mut writer, &context, &resolution)
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    assert_eq!(writer.calls, 1);
}

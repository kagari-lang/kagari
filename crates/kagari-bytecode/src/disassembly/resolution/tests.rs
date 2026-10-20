//! Resolution contracts: verified dispatch plus narrowly constructed index fixtures.

use crate::{
    disassembly::{
        descriptors::References,
        instructions::write_instruction,
        presentation::DisplayContext,
        resolution::{Resolution, TraitSource},
    },
    instruction::{BytecodeInstruction, CallTarget, Slot},
    module::{
        BytecodeFunction, BytecodeModule, FunctionMetadata, FunctionRecord, InterfaceTableRecord,
        RootSlotLayout,
    },
    program::{BytecodeProgram, ModuleRef, verified::VerifiedBytecodeProgram},
};
use kagari_abi::representation::ValueType;
use kagari_common::identity::{DefinitionKind, ModuleIdentity, PackageId, map::DefinitionContext};
use kagari_contract::{
    callable::interface::InterfaceCallContract,
    effects::EffectSet,
    ids::FunctionRef,
    layout::StructLayout,
    native_import::{
        NativeImport,
        callables::{NativeCallableApplication, NativeCallableOrigin},
    },
    types::{ConcreteFunctionIdentity, InterfaceTable, PublicItem, TraitContract},
};
use kagari_types::{
    callable::{CallableImplementation, Signature},
    declaration::{
        FnDecl, NativeDeclaration, Param, TraitDef, requirement::NativeCallableRequirement,
    },
    scalar::BuiltinType,
    ty::{GenericParam, NominalTy, Ty},
};

fn module(name: &str) -> BytecodeModule {
    BytecodeModule {
        identity: ModuleIdentity {
            package: PackageId(name.into()),
            path: vec![],
        },
        ..Default::default()
    }
}

fn integer() -> Ty {
    Ty::Builtin(BuiltinType::I32)
}

fn method(name: &str, receiver: Ty) -> FnDecl {
    FnDecl {
        host_policy: None,
        method_policy: Default::default(),
        name: name.into(),
        implementation: CallableImplementation::Required,
        generic_params: vec![],
        bounds: vec![],
        params: vec![Param {
            name: "self".into(),
            ty: receiver,
            mutable: false,
        }],
        return_type: integer(),
    }
}

#[test]
fn verified_dynamic_dispatch_resolves_declared_member_and_signature_without_an_impl() {
    for private in [false, true] {
        let context = DefinitionContext::new().unwrap();
        let mut owner = module("owner");
        let root = context.intern_root(&owner.identity).unwrap();
        let declaration = context
            .intern_child(root, DefinitionKind::Trait, "Reader", 0)
            .unwrap();
        let interface = NominalTy {
            declaration,
            arguments: vec![],
            associated_types: Default::default(),
        };
        let abi = TraitDef {
            associated_consts: vec![],
            name: "Reader".into(),
            supertraits: vec![],
            generic_params: vec![],
            bounds: vec![],
            methods: vec![
                method("zeta", Ty::SelfType(declaration)),
                method("alpha", Ty::SelfType(declaration)),
            ],
            associated_types: vec![],
            conversion_adapter: None,
        };
        let contract = TraitContract {
            declaration,
            abi: abi.clone(),
        };
        if private {
            owner.trait_contracts.push(contract.clone());
        } else {
            owner.public_items.push(PublicItem::Trait(abi));
        }
        let mut caller = module("caller");
        caller.dependencies.push(ModuleRef::new(1));
        caller.trait_references.push(contract);
        caller.types = vec![ValueType::HeapObject, ValueType::I32];
        let mut metadata = FunctionMetadata {
            params: vec![ValueType::HeapObject],
            return_type: ValueType::I32,
            slots: [
                vec![ValueType::HeapObject, ValueType::I32],
                vec![ValueType::HeapObject],
            ]
            .concat(),
            ..Default::default()
        };
        metadata.roots = RootSlotLayout::from_types(&metadata.slots);
        metadata
            .semantic
            .params
            .insert(0, Ty::Trait(interface.clone()));
        metadata
            .semantic
            .slots
            .insert(0, Ty::Trait(interface.clone()));
        metadata
            .semantic
            .slots
            .insert(2, Ty::Trait(interface.clone()));
        metadata.semantic.slots.insert(1, integer());
        metadata.semantic.result = Some(integer());
        let call = BytecodeInstruction::Call {
            dst: Some(Slot::new(1)),
            callee: CallTarget::InterfaceMethod {
                module: ModuleRef::new(1),
                contract: Box::new(InterfaceCallContract {
                    normalizations: vec![],
                    receiver: None,
                    operations: vec![],
                    interface,
                    method_slot: 1,
                    arguments: vec![],
                }),
            },
            args: vec![Slot::new(0)],
        };
        caller.function_table.push(FunctionRecord {
            id: FunctionRef::new(0),
            identity: None,
            name: "dispatch".into(),
            params: metadata.params.clone(),
            return_type: metadata.return_type,
            effects: metadata.effects,
        });
        caller.functions.push({
            let parameter_base: usize = 2;
            let fixture_instructions = vec![
                BytecodeInstruction::Move {
                    dst: Slot::new(0),
                    src: Slot::new(parameter_base),
                },
                call,
                BytecodeInstruction::Return(Some(Slot::new(1))),
            ];
            let mut fixture_metadata: crate::module::FunctionMetadata = metadata;
            if fixture_metadata.parameter_slots.is_empty() {
                fixture_metadata.parameter_slots =
                    (0..1).map(|i| Slot::new(parameter_base + i)).collect();
            }
            if fixture_metadata.origins.is_empty() {
                fixture_metadata.origins = (0..fixture_instructions.len())
                    .map(|instruction| crate::module::InstructionOrigin {
                        span: Default::default(),
                        line: None,
                        column: None,
                        instruction,
                    })
                    .collect();
            }
            BytecodeFunction {
                id: FunctionRef::new(0),
                identity: None,
                name: "dispatch".into(),
                parameter_count: 1,
                slot_count: u32::try_from(fixture_metadata.slots.len()).unwrap(),
                metadata: fixture_metadata,
                instructions: fixture_instructions,
            }
        });
        let verified = VerifiedBytecodeProgram::new(
            BytecodeProgram {
                root: ModuleRef::new(0),
                modules: vec![caller, owner],
                exports: Default::default(),
            },
            &context,
            &Default::default(),
        )
        .unwrap();
        let program = verified.program();
        let resolution = Resolution::new(program, verified.definitions());
        let display = DisplayContext {
            definitions: verified.definitions(),
        };
        let reference = resolution.trait_reference(1, declaration);
        assert_eq!(
            reference.source,
            if private {
                TraitSource::Contract
            } else {
                TraitSource::PublicItem
            }
        );
        let mut output = Vec::new();
        References::collect(&resolution, &[(0, 0), (0, 0)])
            .write(&mut output, &display, &resolution)
            .unwrap();
        write_instruction(
            &mut output,
            &display,
            &resolution,
            0,
            &program.modules[0].functions[0].instructions[1],
        )
        .unwrap();
        let output = String::from_utf8(output).unwrap();
        assert_eq!(output.matches(" trait owner::Reader").count(), 1);
        assert!(output.contains("method_slot=1 member=owner::Reader::alpha"));
        assert!(output.contains("declared_signature receiver_access=none"));
        assert!(output.contains("param#0 name=\"self\" type=Self<owner::Reader> mutable=false"));
        assert!(output.contains("result=i32"));
        assert!(output.contains(&format!(
            "declaration={reference}, method_slot=1, member=owner::Reader::alpha"
        )));
        assert!(
            !output.contains("interface#"),
            "dynamic dispatch must not invent an impl table"
        );
    }
}

#[test]
fn indexes_preserve_owner_native_signature_canonical_table_and_local_layout_selection() {
    let context = DefinitionContext::new().unwrap();
    let mut caller = module("caller");
    let mut owner = module("owner");
    let root = context.intern_root(&owner.identity).unwrap();
    let entry = context
        .intern_child(root, DefinitionKind::Function, "entry", 0)
        .unwrap();
    let interface_id = context
        .intern_child(root, DefinitionKind::Trait, "Read", 0)
        .unwrap();
    let member = context
        .intern_child(interface_id, DefinitionKind::Method, "read", 0)
        .unwrap();
    let table = context
        .intern_child(root, DefinitionKind::Impl, "table", 0)
        .unwrap();
    let structure = context
        .intern_child(root, DefinitionKind::Struct, "Value", 0)
        .unwrap();
    let interface = NominalTy {
        declaration: interface_id,
        arguments: vec![],
        associated_types: Default::default(),
    };
    let instance = ConcreteFunctionIdentity {
        declaration: entry,
        arguments: vec![],
    };
    let signature = Signature {
        params: vec![integer()],
        result: integer(),
    };
    let import = NativeImport {
        receiver_access: None,
        result_adapter: None,
        generic: None,
        instance: instance.clone(),
        binding: entry,
        signature: signature.clone(),
        requirements: vec![],
        callables: vec![],
    };
    caller.native_imports.push(import.clone());
    let mut wrong_signature = import.clone();
    wrong_signature.signature.result = Ty::Builtin(BuiltinType::Bool);
    owner.native_imports.extend([wrong_signature, import]);
    owner.native_declarations.push(NativeDeclaration {
        declaration: entry,
        concrete_result: None,
        callable_requirements: vec![],
        function: FnDecl {
            name: "entry".into(),
            implementation: CallableImplementation::Native(entry),
            ..method("read", integer())
        },
    });
    let parameter = GenericParam {
        owner: table,
        position: 0,
    };
    owner
        .public_items
        .push(PublicItem::InterfaceTable(Box::new(InterfaceTable {
            associated_type_families: vec![],
            associated_consts: vec![],
            bridge: None,
            declaration: table,
            name: "table".into(),
            generic_params: vec![parameter.clone()],
            bounds: vec![],
            trait_type: Ty::Trait(interface.clone()),
            for_type: parameter.as_type(),
            methods: vec![],
        })));
    for arguments in [vec![], vec![integer()], vec![parameter.as_type()]] {
        owner.interface_tables.push(InterfaceTableRecord {
            declaration: table,
            arguments,
            methods: vec![],
            parents: vec![],
            view: None,
        });
    }
    let layout = StructLayout {
        declaration: structure,
        arguments: vec![],
        fields: vec![],
    };
    caller.structures.push(layout.clone());
    owner.structures.extend([layout.clone(), layout]);
    owner.functions.push(BytecodeFunction {
        identity: Some(instance.clone()),
        ..Default::default()
    });
    let program = BytecodeProgram {
        root: ModuleRef::new(0),
        modules: vec![caller, owner],
        exports: Default::default(),
    };
    let definitions = context.snapshot();
    let resolution = Resolution::new(&program, &definitions);
    let call = NativeCallableApplication {
        origin: NativeCallableOrigin::Implementation,
        requirement: NativeCallableRequirement {
            receiver: integer(),
            interface,
            member,
            arguments: vec![],
        },
        instance: instance.clone(),
        implementation: CallableImplementation::Native(entry),
        signature,
        effects: EffectSet::native_call(),
    };
    assert_eq!(resolution.native(&call), Some((1, 1)));
    assert_eq!(resolution.script(&instance), (1, 0));
    assert_eq!(
        resolution.interface(&ConcreteFunctionIdentity {
            declaration: table,
            arguments: vec![integer()]
        }),
        (1, 1)
    );
    assert_eq!(
        resolution.interface(&ConcreteFunctionIdentity {
            declaration: table,
            arguments: vec![Ty::Parameter {
                owner: entry,
                position: 0
            }]
        }),
        (1, 2)
    );
    let nominal = NominalTy {
        declaration: structure,
        arguments: vec![],
        associated_types: Default::default(),
    };
    assert_eq!(resolution.structure(0, &nominal), Some((0, 0)));
    assert_eq!(resolution.structure(1, &nominal), Some((1, 0)));
    // Repeat lookup against the same output-owned indexes, without rebuilding them.
    assert_eq!(resolution.native(&call), Some((1, 1)));
}

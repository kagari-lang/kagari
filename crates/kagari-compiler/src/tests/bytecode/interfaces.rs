use crate::{source::program::lower_program_to_mir, tests::bytecode::*};
use {
    kagari_bytecode::instruction::{
        BytecodeInstruction, CallTarget as KagaribytecodeCallTarget, Register,
    },
    kagari_mir::instruction::{CallTarget, Instruction},
};

use kagari_bytecode::{module::CallableTarget, program::verify_program};
use kagari_contract::{ids::FunctionRef, types as abi};
use kagari_mir::program as mir_program;

fn script_target(target: CallableTarget) -> FunctionRef {
    let CallableTarget::Script(function) = target else {
        panic!("expected script slot")
    };
    function
}

#[test]
fn applied_trait_bounds_change_public_abi_fingerprint() {
    let fingerprint = |argument: &str| {
        let module = common::bytecode_ok(&format!(
            "pub trait Echo<T> {{}} pub struct Bag<T: Echo<{argument}>> {{ val value: T }} fn main() {{}}"
        ));
        let bag = module.modules[module.root.index()]
            .public_items
            .iter()
            .find_map(|item| match item {
                PublicItem::Type(item) if item.name == "Bag" => Some(item),
                _ => None,
            })
            .unwrap();
        assert!(matches!(&bag.bounds[0].constraints[0],
            abi::Constraint::Trait(ty) if ty.arguments.len() == 1));
        let artifact = KbcArtifact::from_program(module, ArtifactBuildOptions::default()).unwrap();
        artifact
            .verification
            .public_abi_fingerprints
            .into_iter()
            .find(|item| item.name == "type:Bag")
            .unwrap()
            .fingerprint
    };
    assert_ne!(fingerprint("i32"), fingerprint("String"));
}

#[test]
fn applied_trait_bound_rejects_a_foreign_binder_before_loading() {
    let mut module = common::bytecode_ok(
        "pub trait Echo<T> {} pub struct Bag<T: Echo<T>> { val value: T } fn main() {}",
    );
    let bag = module.modules[module.root.index()]
        .public_items
        .iter_mut()
        .find_map(|item| match item {
            PublicItem::Type(item) if item.name == "Bag" => Some(item),
            _ => None,
        })
        .unwrap();
    let kagari_contract::types::Constraint::Trait(ty) = &mut bag.bounds[0].constraints[0] else {
        panic!("trait bound");
    };
    let kagari_contract::types::Ty::Parameter { owner, .. } = &mut ty.arguments[0] else {
        panic!("template argument");
    };
    *owner = ty.declaration.clone();
    assert!(matches!(
        verify_program(&module),
        Err(BytecodeVerificationError::InvalidPublicAbi)
    ));
    assert!(matches!(
        KbcArtifact::from_program(module, ArtifactBuildOptions::default(),),
        Err(ArtifactValidationError::Bytecode(_))
    ));
}

#[test]
fn applied_trait_interface_table_preserves_method_contract() {
    let module = common::bytecode_ok(
        "pub trait Echo<T> { fn get(self) -> T; } pub struct Pair { val number: i32 } impl Echo<i32> for Pair { fn get(self) -> i32 { self.number } } fn main() {}",
    );
    assert_eq!(
        module.modules[module.root.index()].interface_tables.len(),
        1
    );
    assert_eq!(
        module.modules[module.root.index()].interface_tables[0]
            .methods
            .len(),
        1
    );
    verify_program(&module).unwrap();
    let artifact = KbcArtifact::from_program(module, ArtifactBuildOptions::default()).unwrap();
    KbcArtifact::from_bytes(&artifact.to_bytes().unwrap())
        .unwrap()
        .validate_for_loader(&ArtifactCompatibility::default())
        .unwrap();
}

#[test]
fn applied_trait_method_bounds_match_across_binder_owners() {
    let module = common::bytecode_ok(
        "pub trait Marker<T> {} pub struct Holder {} impl Marker<i32> for Holder {} pub trait Consumer<T> { fn take<U: Marker<T>>(self, value: U) -> U; } impl Consumer<i32> for Holder { fn take<V: Marker<i32>>(self, value: V) -> V { value } } fn main() {}",
    );
    verify_program(&module).unwrap();
}

#[test]
fn applied_trait_template_keeps_impl_and_trait_arguments() {
    let module = common::bytecode_ok(
        "pub trait Echo<T> { fn get(self) -> T; } pub struct Holder<T> { val value: T } impl<T> Echo<T> for Holder<T> { fn get(self) -> T { self.value } } fn main() {}",
    );
    assert_eq!(
        module.modules[module.root.index()].interface_tables.len(),
        1
    );
    assert!(
        module.modules[module.root.index()].interface_tables[0]
            .methods
            .is_empty()
    );
    assert!(module.modules[module.root.index()].public_items.iter().any(|item| matches!(item,
        PublicItem::InterfaceTable(table)
            if table.generic_params.len() == 1
                && matches!(&table.trait_type, abi::Ty::Trait(instance) if instance.arguments.len() == 1)
    )));
}

#[test]
fn generic_interface_implementation_specializes_reachable_method() {
    let module = common::bytecode_ok(
        "pub trait Get { fn get(self) -> i32; } pub struct Holder<T> { val value: T } impl<T> Get for Holder<T> { fn get(self) -> i32 { 42 } } fn read(x: Get) -> i32 { x.get() } fn main() -> (i32, i32) { (read(Holder { value: 1 }), read(Holder { value: \"a\" })) }",
    );
    assert_eq!(
        module.modules[module.root.index()].interface_tables.len(),
        3
    );
    let slots = module.modules[module.root.index()]
        .interface_tables
        .iter()
        .flat_map(|table| &table.methods)
        .collect::<Vec<_>>();
    assert_eq!(slots.len(), 2);
    let abi = module.modules[module.root.index()]
        .public_items
        .iter()
        .find_map(|item| match item {
            PublicItem::InterfaceTable(table) => Some(table),
            _ => None,
        })
        .unwrap();
    assert_eq!(abi.generic_params.len(), 1);
    assert!(abi.methods[0].generic_params.is_empty());
    let arguments = slots
        .iter()
        .map(|slot| {
            module.modules[module.root.index()].functions[script_target(slot.target).index()]
                .identity
                .as_ref()
                .unwrap()
                .arguments
                .clone()
        })
        .collect::<Vec<_>>();
    assert!(arguments.contains(&vec![abi::Ty::Builtin(
        kagari_contract::scalar::BuiltinType::I32
    )]));
    assert!(arguments.contains(&vec![abi::Ty::Builtin(
        kagari_contract::scalar::BuiltinType::String
    )]));
    let mut wrong_arity = module.clone();
    let method = slots[0].target;
    let method = script_target(method).index();
    wrong_arity.modules[wrong_arity.root.index()].functions[method]
        .identity
        .as_mut()
        .unwrap()
        .arguments
        .clear();
    wrong_arity.modules[wrong_arity.root.index()].function_table[method].identity =
        wrong_arity.modules[wrong_arity.root.index()].functions[method]
            .identity
            .clone();
    assert!(matches!(
        verify_program(&wrong_arity),
        Err(BytecodeVerificationError::InvalidInterfaceTable)
    ));
}

#[test]
fn generic_interface_slot_requires_instantiated_method_layout() {
    let module = common::bytecode_ok(
        "pub trait Echo<T> { fn get(self) -> T; } pub struct Holder<T> { val value: T } impl<T> Echo<T> for Holder<T> { fn get(self) -> T { self.value } } fn read(x: Echo<i32>) -> i32 { x.get() } fn main() -> i32 { read(Holder { value: 7 }) }",
    );
    assert_eq!(
        module.modules[module.root.index()]
            .interface_tables
            .iter()
            .map(|table| table.methods.len())
            .sum::<usize>(),
        1
    );
    verify_program(&module).unwrap();
    let mut wrong_instance = module;
    let method = wrong_instance.modules[wrong_instance.root.index()]
        .interface_tables
        .iter()
        .flat_map(|table| &table.methods)
        .next()
        .unwrap()
        .target;
    let method = script_target(method).index();
    wrong_instance.modules[wrong_instance.root.index()].functions[method]
        .identity
        .as_mut()
        .unwrap()
        .arguments[0] =
        kagari_contract::types::Ty::Builtin(kagari_contract::scalar::BuiltinType::Bool);
    wrong_instance.modules[wrong_instance.root.index()].function_table[method].identity =
        wrong_instance.modules[wrong_instance.root.index()].functions[method]
            .identity
            .clone();
    assert!(matches!(
        verify_program(&wrong_instance),
        Err(BytecodeVerificationError::InvalidInterfaceTable)
    ));
}

#[test]
fn concrete_interface_methods_have_verified_executable_slots() {
    let module = common::bytecode_ok(
        "pub struct Pair { val number: i32 } pub trait Number { fn get(self) -> i32; } impl Number for Pair { fn get(self) -> i32 { self.number } } fn main() -> i32 { 1 }",
    );
    assert_eq!(
        module.modules[module.root.index()].interface_tables.len(),
        1
    );
    let table = &module.modules[module.root.index()].interface_tables[0];
    assert_eq!(table.methods.len(), 1);
    let slot = &table.methods[0];
    assert_eq!(slot.method.path.last().unwrap().name, "get");
    let function =
        &module.modules[module.root.index()].functions[script_target(slot.target).index()];
    let identity = function.identity.as_ref().unwrap();
    assert_eq!(identity.declaration.path.last().unwrap().name, "get");
    assert_eq!(
        identity.declaration.path[0].kind,
        kagari_common::identity::DefinitionKind::Impl
    );
    assert!(identity.arguments.is_empty());
    verify_program(&module).unwrap();

    let artifact =
        KbcArtifact::from_program(module.clone(), ArtifactBuildOptions::default()).unwrap();
    let decoded = KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    decoded
        .validate_for_loader(&ArtifactCompatibility::default())
        .unwrap();
    assert_eq!(
        decoded.program.modules[decoded.program.root.index()].interface_tables[0].methods[0].target,
        slot.target
    );

    let mut missing = module.clone();
    missing.modules[missing.root.index()].interface_tables[0]
        .methods
        .clear();
    assert!(matches!(
        verify_program(&missing),
        Err(BytecodeVerificationError::InvalidInterfaceTable)
    ));
    let mut wrong_target = module.clone();
    wrong_target.modules[wrong_target.root.index()].interface_tables[0].methods[0].target =
        CallableTarget::Script(
            wrong_target.modules[wrong_target.root.index()]
                .functions
                .iter()
                .find(|function| function.name == "main")
                .unwrap()
                .id,
        );
    assert!(matches!(
        verify_program(&wrong_target),
        Err(BytecodeVerificationError::InvalidInterfaceTable)
    ));
    let mut wrong_method = module.clone();
    wrong_method.modules[wrong_method.root.index()].interface_tables[0].methods[0]
        .method
        .path
        .last_mut()
        .unwrap()
        .name = "other".into();
    assert!(matches!(
        verify_program(&wrong_method),
        Err(BytecodeVerificationError::InvalidInterfaceTable)
    ));
    let mut missing_table = module;
    missing_table.modules[missing_table.root.index()]
        .interface_tables
        .clear();
    assert!(matches!(
        verify_program(&missing_table),
        Err(BytecodeVerificationError::InvalidInterfaceTable)
    ));
}

#[test]
fn source_interface_coercion_links_an_imported_implementation_table() {
    use {
        kagari_common::identity::{ModuleIdentity, PackageId},
        kagari_source::source_database::{SourceDatabase, SourceLayer},
    };

    let mut sources = SourceDatabase::default();
    let mut root = None;
    for (name, source) in [
        (
            "dependency",
            "pub trait Tag { fn tag(self) -> i32; } impl Tag for i32 { fn tag(self) -> i32 { self + 1 } }",
        ),
        (
            "root",
            "use pkg::dependency::Tag; fn accept(value: Tag) -> i32 { value.tag() } fn main() -> i32 { accept(7) }",
        ),
    ] {
        let uri = format!("mem://{name}");
        sources
            .bind_module(
                &uri,
                ModuleIdentity {
                    package: PackageId("pkg".into()),
                    path: vec![name.into()],
                },
            )
            .unwrap();
        let revision = sources.set(&uri, source.into(), SourceLayer::Base).unwrap();
        if name == "root" {
            root = Some(revision);
        }
    }
    let snapshot = kagari_hir::analysis::AnalysisDatabase::default()
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let checked = snapshot
        .check_program(root.unwrap(), &Default::default())
        .unwrap();
    let ir = lower_program_to_mir(&checked, &Default::default()).unwrap();
    let mut forged = ir.clone().into_unverified();
    let root_ir = forged
        .iter_mut()
        .find(|module| module.identity.path == ["root"])
        .unwrap();
    let implementation = root_ir
        .functions
        .iter_mut()
        .flat_map(|function| &mut function.blocks)
        .flat_map(|block| &mut block.instructions)
        .find_map(|instruction| match instruction {
            Instruction::MakeInterface { implementation, .. } => Some(implementation),
            _ => None,
        })
        .unwrap();
    implementation.path.last_mut().unwrap().name = "forged".into();
    assert!(matches!(
        mir_program::verify_program(ir.root().clone(), forged, &Default::default()),
        Err(mir_program::ProgramError {
            kind: mir_program::ProgramErrorKind::InterfaceContract(_),
            ..
        })
    ));
    let mut forged_call = ir.clone().into_unverified();
    let method_slot = forged_call
        .iter_mut()
        .find(|module| module.identity.path == ["root"])
        .unwrap()
        .functions
        .iter_mut()
        .flat_map(|function| &mut function.blocks)
        .flat_map(|block| &mut block.instructions)
        .find_map(|instruction| match instruction {
            Instruction::Call {
                callee: CallTarget::InterfaceMethod(contract),
                ..
            } => Some(&mut contract.method_slot),
            _ => None,
        })
        .unwrap();
    *method_slot = 99;
    assert!(matches!(
        mir_program::verify_program(ir.root().clone(), forged_call, &Default::default()),
        Err(mir_program::ProgramError {
            kind: mir_program::ProgramErrorKind::InterfaceContract(_),
            ..
        })
    ));
    let bytecode = crate::bytecode::lower_program_to_bytecode(&ir).unwrap();
    let root_module = &bytecode.modules[bytecode.root.index()];
    assert!(root_module.functions.iter().flat_map(|function| &function.instructions).any(
        |instruction| matches!(instruction, BytecodeInstruction::MakeInterface { module, .. } if module.index() != bytecode.root.index())
    ));
    assert!(root_module.functions.iter().flat_map(|function| &function.instructions).any(
        |instruction| matches!(instruction, BytecodeInstruction::Call { callee: KagaribytecodeCallTarget::InterfaceMethod { module, .. }, .. } if module.index() != bytecode.root.index())
    ));
    verify_program(&bytecode).unwrap();
    let mut invalid = bytecode.clone();
    let call = invalid.modules[bytecode.root.index()]
        .functions
        .iter_mut()
        .flat_map(|function| &mut function.instructions)
        .find_map(|instruction| match instruction {
            BytecodeInstruction::Call {
                callee: KagaribytecodeCallTarget::InterfaceMethod { contract, .. },
                ..
            } => Some(&mut contract.method_slot),
            _ => None,
        })
        .unwrap();
    *call = 99;
    assert!(verify_program(&invalid).is_err());
    let mut wrong_owner = bytecode.clone();
    let owner_slot = wrong_owner.modules[bytecode.root.index()]
        .functions
        .iter_mut()
        .flat_map(|function| &mut function.instructions)
        .find_map(|instruction| match instruction {
            BytecodeInstruction::Call {
                callee: KagaribytecodeCallTarget::InterfaceMethod { module, .. },
                ..
            } => Some(module),
            _ => None,
        })
        .unwrap();
    *owner_slot = bytecode.root;
    assert!(verify_program(&wrong_owner).is_err());
}

#[test]
fn forged_interface_method_slots_are_rejected_before_execution() {
    let original = common::bytecode_ok(
        "trait Tag { fn tag(self) -> i32; } impl Tag for i32 { fn tag(self) -> i32 { self } } fn read(value: Tag) -> i32 { value.tag() } fn main() -> i32 { read(7) }",
    );
    verify_program(&original).unwrap();
    for corruption in ["slot", "owner", "argument"] {
        let mut forged = original.clone();
        let function = forged.modules[forged.root.index()]
            .functions
            .iter_mut()
            .find(|function| function.name == "read")
            .unwrap();
        let instruction = function
            .instructions
            .iter_mut()
            .find(|instruction| {
                matches!(
                    instruction,
                    BytecodeInstruction::Call {
                        callee: KagaribytecodeCallTarget::InterfaceMethod { .. },
                        ..
                    }
                )
            })
            .unwrap();
        let BytecodeInstruction::Call { callee, args, .. } = instruction else {
            unreachable!()
        };
        let KagaribytecodeCallTarget::InterfaceMethod { contract, .. } = callee else {
            unreachable!()
        };
        match corruption {
            "slot" => contract.method_slot = 99,
            "owner" => {
                contract.interface.declaration.path.last_mut().unwrap().name = "Other".into()
            }
            "argument" => args[0] = Register::new(999),
            _ => unreachable!(),
        }
        assert!(verify_program(&forged).is_err(), "accepted {corruption}");
    }
}

#[test]
fn private_interface_tables_must_match_their_trait_contract() {
    use kagari_contract::{scalar::BuiltinType, types::Ty};

    let original = common::bytecode_ok(
        "trait Readable { fn get(self) -> i32; } struct Counter { val value: i32 } impl Readable for Counter { fn get(self) -> i32 { self.value } } fn main() {}",
    );
    let index = original.modules[original.root.index()]
        .public_items
        .iter()
        .position(|item| matches!(item, PublicItem::InterfaceTable(_)))
        .unwrap();
    assert_eq!(
        original.modules[original.root.index()]
            .trait_contracts
            .len(),
        1
    );
    verify_program(&original).unwrap();
    for corruption in ["result", "roster", "missing trait"] {
        let mut forged = original.clone();
        match corruption {
            "result" => {
                let PublicItem::InterfaceTable(table) =
                    &mut forged.modules[forged.root.index()].public_items[index]
                else {
                    unreachable!()
                };
                table.methods[0].return_type = Ty::Builtin(BuiltinType::Bool);
            }
            "roster" => {
                let PublicItem::InterfaceTable(table) =
                    &mut forged.modules[forged.root.index()].public_items[index]
                else {
                    unreachable!()
                };
                table.methods.clear();
            }
            "missing trait" => forged.modules[forged.root.index()].trait_contracts.clear(),
            _ => unreachable!(),
        }
        assert!(
            matches!(
                verify_program(&forged),
                Err(BytecodeVerificationError::InvalidPublicAbi)
            ),
            "{corruption}"
        );
    }
}

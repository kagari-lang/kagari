//! Checked resume-body IR before source async syntax is enabled.
use crate::{bytecode::lower_program_to_bytecode, tests::common};
use kagari_abi::representation::ValueType;
use kagari_bytecode::{artifact::KbcArtifact, instruction::BytecodeInstruction};
use kagari_common::identity::DefinitionKind;
use kagari_contract::types::PublicItem;
use kagari_mir::{
    codec::{decode_program, encode_program},
    function::{MirModule, MirTemp},
    ids::{LocalId, TempId},
    instruction::{Instruction, MirValue},
    program::{VerifiedMirProgram, verify_program},
    verify::verify_mir,
};
use kagari_types::{
    declaration::{TypeDefKind, native::NativeStorageLayout},
    ty::Ty,
};

fn program() -> (Vec<MirModule>, usize) {
    let program =
        common::mir_ok("pub struct Future<T> {} fn wait(value: Future<i32>) -> i32 { 0 }");
    let root = program.root().clone();
    let mut modules = program.into_unverified();
    let index = modules
        .iter()
        .position(|module| module.identity == root)
        .unwrap();
    let module = &mut modules[index];
    for item in &mut module.abi.public_items {
        if let PublicItem::Type(ty) = item {
            assert_eq!(ty.name, "Future");
            ty.kind = TypeDefKind::NativeStorage(NativeStorageLayout::Future);
            ty.generic_params[0].owner.path.last_mut().unwrap().kind =
                DefinitionKind::AssociatedType;
        }
    }
    module.structures.clear();
    let function = module
        .functions
        .iter_mut()
        .find(|function| function.name == "wait")
        .unwrap();
    let Ty::Struct(mut nominal) = function.semantic.params[&0].clone() else {
        panic!("Future parameter");
    };
    nominal.declaration.path.last_mut().unwrap().kind = DefinitionKind::AssociatedType;
    let future = Ty::NativeObject(nominal);
    function.semantic.params.insert(0, future.clone());
    function.semantic.locals.insert(0, future.clone());
    let value = MirValue {
        temp: TempId::new(function.temps.len()),
        ty: ValueType::HeapObject,
    };
    function.temps.push(MirTemp { ty: value.ty });
    function
        .semantic
        .registers
        .insert(value.temp.index(), future.clone());
    let block = &mut function.blocks[function.entry.index()];
    let Instruction::LoadConst { dst, .. } = block.instructions[0] else {
        panic!("constant return");
    };
    block.instructions[0] = Instruction::LoadLocal {
        dst: value,
        local: LocalId::new(0),
    };
    block
        .instructions
        .push(Instruction::Await { dst, value, future });
    block.instruction_spans.push(block.instruction_spans[0]);
    block.instruction_scopes.push(block.instruction_scopes[0]);
    for instruction in &block.instructions {
        function.effects = function.effects.union(instruction.effects());
    }
    for item in &mut module.abi.public_items {
        if let PublicItem::Function(declaration) = item {
            declaration.params[0].ty = function.semantic.params[&0].clone();
        }
    }
    (modules, index)
}

fn verified(modules: Vec<MirModule>, root: usize) -> VerifiedMirProgram {
    verify_program(modules[root].identity.clone(), modules, &Default::default()).unwrap()
}

#[test]
fn async_mir_suspension_contract() {
    let (modules, root) = program();
    let verified = verified(modules.clone(), root);
    let bytes = encode_program(&verified, &Default::default()).unwrap();
    let decoded = decode_program(&bytes, &Default::default()).unwrap();
    let bytecode = lower_program_to_bytecode(&decoded).unwrap();
    assert!(
        bytecode
            .modules
            .iter()
            .flat_map(|module| &module.functions)
            .flat_map(|function| &function.instructions)
            .any(|instruction| matches!(instruction, BytecodeInstruction::Await { .. }))
    );
    KbcArtifact::from_program(bytecode, Default::default())
        .unwrap()
        .into_verified(&Default::default())
        .unwrap();
    for mutation in 0..4 {
        let mut module = modules[root].clone();
        let function = module
            .functions
            .iter_mut()
            .find(|function| function.name == "wait")
            .unwrap();
        let block = &mut function.blocks[function.entry.index()];
        match mutation {
            0 => function.effects.may_suspend = false,
            1 => {
                let Instruction::Await { dst, value, .. } = &mut block.instructions[1] else {
                    unreachable!();
                };
                *value = *dst;
            }
            2 => {
                block.instructions.insert(0, Instruction::EndIteration);
                block
                    .instruction_spans
                    .insert(0, block.instruction_spans[0]);
                block
                    .instruction_scopes
                    .insert(0, block.instruction_scopes[0]);
            }
            3 => {
                let PublicItem::Type(ty) = module
                    .abi
                    .public_items
                    .iter_mut()
                    .find(|item| matches!(item, PublicItem::Type(_)))
                    .unwrap()
                else {
                    unreachable!();
                };
                ty.kind = TypeDefKind::NativeStorage(NativeStorageLayout::Opaque);
            }
            _ => unreachable!(),
        }
        assert!(
            verify_mir(module, &Default::default()).is_err(),
            "MIR mutation {mutation}"
        );
    }
}

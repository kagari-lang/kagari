//! Checked resume-body IR before source async syntax is enabled.
use crate::{bytecode::lower_program_to_bytecode, tests::common};
use kagari_abi::representation::ValueType;
use kagari_bytecode::{artifact::KbcArtifact, instruction::BytecodeInstruction};
use kagari_common::identity::{DefinitionKind, map::DefinitionContext};
use kagari_contract::types::PublicItem;
use kagari_mir::{
    codec::{decode_program, encode_program},
    function::{MirModule, MirTemp},
    ids::{LocalId, TempId},
    instruction::{Instruction, MirValue},
    program::{VerifiedMirProgram, records::MirProgram, verify_program},
};
use kagari_types::{
    declaration::{TypeDefKind, native::NativeStorageLayout},
    ty::Ty,
};

fn program(context: &DefinitionContext) -> (Vec<MirModule>, usize) {
    let program =
        common::mir_ok("pub struct Future<T> {} fn wait(value: Future<i32>) -> i32 { 0 }");
    let root = program.root().clone();
    let mut modules = common::adopt(context, program.into_unverified()).modules;
    let index = modules
        .iter()
        .position(|module| module.identity == root)
        .unwrap();
    let module = &mut modules[index];
    for item in &mut module.abi.public_items {
        if let PublicItem::Type(ty) = item {
            assert_eq!(ty.name, "Future");
            ty.kind = TypeDefKind::NativeStorage(NativeStorageLayout::Future);
            common::edit_definition(context, &mut ty.generic_params[0].owner, |path| {
                path.path.last_mut().unwrap().kind = DefinitionKind::AssociatedType
            });
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
    common::edit_definition(context, &mut nominal.declaration, |path| {
        path.path.last_mut().unwrap().kind = DefinitionKind::AssociatedType
    });
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

fn verified(
    context: &DefinitionContext,
    modules: Vec<MirModule>,
    root: usize,
) -> VerifiedMirProgram {
    verify_program(
        MirProgram {
            root: modules[root].identity.clone(),
            modules,
            exports: Default::default(),
        },
        context,
        &Default::default(),
    )
    .unwrap()
}

#[test]
fn async_mir_suspension_contract() {
    let context = DefinitionContext::new().unwrap();
    let (modules, root) = program(&context);
    let verified = verified(&context, modules.clone(), root);
    let bytes = encode_program(&verified, &Default::default()).unwrap();
    let decoded = decode_program(&bytes, &Default::default()).unwrap();
    let bytecode = common::adopt_bytecode(
        &context,
        lower_program_to_bytecode(&decoded, &Default::default())
            .map(|program| program.into_unverified())
            .unwrap(),
    );
    assert!(
        bytecode
            .modules
            .iter()
            .flat_map(|module| &module.functions)
            .flat_map(|function| &function.instructions)
            .any(|instruction| matches!(instruction, BytecodeInstruction::Await { .. }))
    );
    KbcArtifact::from_program(bytecode, Default::default(), &context, &Default::default())
        .unwrap()
        .into_verified(&Default::default(), &context, &Default::default())
        .unwrap();
    for mutation in 0..3 {
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
            common::verify_edited(&context, module).is_err(),
            "MIR mutation {mutation}"
        );
    }
}

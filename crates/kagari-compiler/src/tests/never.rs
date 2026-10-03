use crate::{source::lower::lower_to_mir, tests::common};
use kagari_bytecode::{
    instruction::{BytecodeInstruction, Register},
    program::verify_program,
    verifier::BytecodeVerificationError,
};

use {kagari_abi::representation::ValueType, kagari_contract::contracts::ContractError};

use kagari_mir::{
    ids::TempId,
    instruction::{MirValue, Terminator},
    verify::{MirVerificationErrorKind, verify_mir},
};

#[test]
fn never_functions_reject_even_never_typed_return_operands() {
    let input = common::program_ok("fn impossible(value: !) -> ! { value }");
    let mir = lower_to_mir(input.root(), &Default::default()).unwrap();
    let mut bytecode = common::bytecode_with_edited_root(&input, &mir);
    let mut raw = mir.into_unverified();
    let function = raw
        .functions
        .iter_mut()
        .find(|f| f.name == "impossible")
        .unwrap();
    let temp = function
        .temps
        .iter()
        .position(|t| t.ty == ValueType::Never)
        .unwrap();
    function.blocks[function.entry.index()].terminator = Some(Terminator::Return(Some(MirValue {
        temp: TempId::new(temp),
        ty: ValueType::Never,
    })));
    assert!(matches!(
        verify_mir(raw, &Default::default()).unwrap_err().kind,
        MirVerificationErrorKind::Contract(ContractError::InvalidOperation {
            reason: "Never function cannot return"
        })
    ));

    let function = bytecode.modules[bytecode.root.index()]
        .functions
        .iter_mut()
        .find(|f| f.name == "impossible")
        .unwrap();
    let register = function
        .metadata
        .registers
        .iter()
        .position(|t| *t == ValueType::Never)
        .unwrap();
    let point = function
        .instructions
        .iter_mut()
        .find(|i| matches!(i, BytecodeInstruction::Unreachable))
        .unwrap();
    *point = BytecodeInstruction::Return(Some(Register::new(register)));
    assert!(matches!(
        verify_program(&bytecode).unwrap_err(),
        BytecodeVerificationError::InvalidOperation {
            reason: "Never function cannot return",
            ..
        }
    ));
}

#[test]
fn never_calls_terminate_without_emitting_following_effects() {
    let input = common::program_ok(
        "fn fail() -> ! { fail() } fn effect() {} fn main() -> i32 { fail(); effect(); 42 }",
    );
    let mir = lower_to_mir(input.root(), &Default::default()).unwrap();
    let main = mir.functions.iter().find(|f| f.name == "main").unwrap();
    assert!(
        main.blocks
            .iter()
            .all(|block| !matches!(block.terminator, Some(Terminator::Return(_))))
    );
    let bytecode = common::bytecode_with_edited_root(&input, &mir);
    verify_program(&bytecode).unwrap();
}

use crate::{bytecode::lower_to_bytecode, lower_to_mir, tests::common};
use kagari_abi::{contracts::ContractError, representation::ValueType};
use kagari_bytecode::{self as bytecode, BytecodeInstruction, BytecodeVerificationError, Register};
use kagari_mir::{MirValue, MirVerificationErrorKind, TempId, Terminator, verify_mir};

#[test]
fn never_functions_reject_even_never_typed_return_operands() {
    let mir = lower_to_mir(
        &common::analyze_ok("fn impossible(value: !) -> ! { value }"),
        &Default::default(),
    )
    .unwrap();
    let mut bytecode = lower_to_bytecode(&mir).unwrap();
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

    let function = bytecode
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
        bytecode::verify_module(&bytecode).unwrap_err(),
        BytecodeVerificationError::InvalidOperation {
            reason: "Never function cannot return",
            ..
        }
    ));
}

#[test]
fn never_calls_terminate_without_emitting_following_effects() {
    let mir = lower_to_mir(&common::analyze_ok(
        "fn fail() -> ! { std::debug::panic(\"stop\") } fn main() -> i32 { fail(); std::debug::print(\"unreachable\"); 42 }"
    ), &Default::default()).unwrap();
    let main = mir.functions.iter().find(|f| f.name == "main").unwrap();
    assert!(
        main.blocks
            .iter()
            .all(|block| !matches!(block.terminator, Some(Terminator::Return(_))))
    );
    let bytecode = lower_to_bytecode(&mir).unwrap();
    bytecode::verify_module(&bytecode).unwrap();
}

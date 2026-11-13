//! Checked tail candidates retain their source-free ordinary fallback.
use crate::tests::common;
use kagari_bytecode::{
    artifact::{ArtifactValidationError, KbcArtifact},
    instruction::{BytecodeInstruction as I, CallTarget, JumpTarget},
    program::verify_program,
    verifier::BytecodeVerificationError,
};
use kagari_common::identity::map::DefinitionContext;
use std::mem;

const SOURCE: &str = include_str!("../../../tests/fixtures/scalar-self-tail.kgr");

#[test]
fn scalar_self_tail_candidates_keep_the_verified_terminal_pair() {
    let context = DefinitionContext::new().unwrap();
    let code = common::adopt_bytecode(&context, common::bytecode_ok(SOURCE));
    let root = &code.modules[code.root.index()];
    let sum = root.functions.iter().find(|f| f.name == "sum").unwrap();
    assert_eq!(sum.metadata.self_tail_calls.len(), 1);
    let pc = sum.metadata.self_tail_calls[0];
    assert!(matches!(
        &sum.instructions[pc..pc + 2],
        [I::Call { dst, callee: CallTarget::Function(target), .. }, I::Return(result)]
            if *target == sum.id && dst == result
    ));
    for name in [
        "ordinary",
        "adapted",
        "scalar_loop",
        "merged",
        "managed",
        "peer",
    ] {
        assert!(
            root.functions
                .iter()
                .find(|f| f.name == name)
                .unwrap()
                .metadata
                .self_tail_calls
                .is_empty()
        );
    }
    let artifact = KbcArtifact::from_program(
        code.clone(),
        Default::default(),
        &context,
        &Default::default(),
    )
    .unwrap();
    let bytes = artifact.to_bytes(&context.snapshot()).unwrap();
    let decoded = common::adopt(&context, KbcArtifact::from_bytes(&bytes).unwrap());
    decoded
        .validate_for_loader(&Default::default(), &context, &Default::default())
        .unwrap();
    let decoded_sum = decoded.program.modules[code.root.index()]
        .functions
        .iter()
        .find(|function| function.name == "sum")
        .unwrap();
    assert_eq!(
        decoded_sum.metadata.self_tail_calls,
        sum.metadata.self_tail_calls
    );
}

#[derive(Clone, Copy, Debug)]
enum Forgery {
    OutOfBounds,
    Duplicate,
    NonCall,
    NonSelf,
    DifferentResult,
    AlternateReturnEntry,
    Managed,
}

#[test]
fn forged_tail_candidates_reach_source_free_validation() {
    let context = DefinitionContext::new().unwrap();
    let valid = common::adopt_bytecode(&context, common::bytecode_ok(SOURCE));
    for forgery in [
        Forgery::OutOfBounds,
        Forgery::Duplicate,
        Forgery::NonCall,
        Forgery::NonSelf,
        Forgery::DifferentResult,
        Forgery::AlternateReturnEntry,
        Forgery::Managed,
    ] {
        let mut forged = valid.clone();
        let module = &mut forged.modules[forged.root.index()];
        let peer = module
            .functions
            .iter()
            .find(|f| f.name == "peer")
            .unwrap()
            .id;
        let name = if matches!(forgery, Forgery::Managed) {
            "managed"
        } else {
            "sum"
        };
        let function = module
            .functions
            .iter_mut()
            .find(|f| f.name == name)
            .unwrap();
        let pc = function
            .instructions
            .iter()
            .position(|i| matches!(i, I::Call { .. }))
            .unwrap();
        match forgery {
            Forgery::OutOfBounds => function.metadata.self_tail_calls = vec![usize::MAX],
            Forgery::Duplicate => function.metadata.self_tail_calls = vec![pc, pc],
            Forgery::NonCall => function.metadata.self_tail_calls = vec![pc + 1],
            Forgery::NonSelf => {
                let I::Call { callee, .. } = &mut function.instructions[pc] else {
                    unreachable!()
                };
                *callee = CallTarget::Function(peer);
            }
            Forgery::DifferentResult => {
                function.instructions[pc + 1] =
                    I::Return(Some(function.metadata.parameter_slots[0]));
            }
            Forgery::AlternateReturnEntry => {
                let parameter = function.metadata.parameter_slots[0];
                let I::Call { dst, .. } = &mut function.instructions[pc] else {
                    unreachable!()
                };
                *dst = Some(parameter);
                function.instructions[pc + 1] = I::Return(Some(parameter));
                let branch = function
                    .instructions
                    .iter_mut()
                    .find(|i| matches!(i, I::Branch { .. } | I::PredicateBranch { .. }))
                    .unwrap();
                let (I::Branch { then_target, .. } | I::PredicateBranch { then_target, .. }) =
                    branch
                else {
                    unreachable!()
                };
                // Leave the optional summary untouched: actual edges are authority.
                *then_target = JumpTarget::new(pc + 1);
            }
            Forgery::Managed => function.metadata.self_tail_calls = vec![pc],
        }
        let expected = BytecodeVerificationError::InvalidOperation {
            function: function.id,
            reason: "invalid scalar self-tail candidate",
        };
        // The same instruction stream must independently pass ordinary admission;
        // a malformed call/flow must not make this negative test pass accidentally.
        let candidates = mem::take(&mut function.metadata.self_tail_calls);
        common::project_bytecode(&context, &mut forged);
        verify_program(&forged, &context.snapshot(), &Default::default()).unwrap();
        forged.modules[forged.root.index()]
            .functions
            .iter_mut()
            .find(|f| f.name == name)
            .unwrap()
            .metadata
            .self_tail_calls = candidates;
        assert_eq!(
            verify_program(&forged, &context.snapshot(), &Default::default()),
            Err(expected.clone()),
            "{forgery:?}"
        );
        let mut artifact = KbcArtifact::from_program(
            valid.clone(),
            Default::default(),
            &context,
            &Default::default(),
        )
        .unwrap();
        artifact.program = forged;
        let decoded = common::adopt(
            &context,
            KbcArtifact::from_bytes(&artifact.to_bytes(&context.snapshot()).unwrap()).unwrap(),
        );
        assert!(
            matches!(decoded.validate_for_loader(&Default::default(), &context, &Default::default()),
            Err(ArtifactValidationError::Bytecode(error)) if error == expected),
            "{forgery:?}"
        );
    }
}

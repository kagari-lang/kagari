use super::{SOURCE, registration};
use crate::tests::execution::native_boundary::{
    native_boundary_functions::fixture, native_boundary_payload::PlayerState,
};
use kagari_common::cancellation::CancellationToken;
use kagari_runtime::{context::ExecutionContext, error::RuntimeErrorKind, native::payload::Opaque};

#[test]
fn opaque_receiver_loans_release_after_depth_exhaustion_and_cancellation() {
    let (module, _, _) = registration();
    let (mut vm, program) = fixture(SOURCE, Some(&module));
    let owner = vm.select_program(&program).unwrap();
    let player: Opaque<PlayerState> = vm
        .execute_typed(&program, "make", (), &Default::default())
        .unwrap();

    let error = vm
        .execute_typed::<_, i32>(&program, "recurse", (player.clone(),), &Default::default())
        .unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::StackOverflow);
    assert_eq!(vm.resources().counters().current_call_depth, 0);
    let mut cx = vm.context(&owner).unwrap();
    let hp = player.read(&mut cx, |state| Ok(state.hp)).unwrap();
    assert!(hp < 100, "completed receiver writes must survive the trap");
    let vm = cx.runtime_mut();

    let cancellation = CancellationToken::default();
    cancellation.cancel();
    let cancelled = ExecutionContext {
        cancellation,
        ..Default::default()
    };
    let error = vm
        .execute_typed::<_, i32>(&program, "hit", (player.clone(), 1), &cancelled)
        .unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::Cancelled);
    assert_eq!(vm.resources().counters().current_call_depth, 0);
    assert_eq!(
        vm.execute_typed::<_, String>(&program, "inspect", (player.clone(),), &Default::default())
            .unwrap(),
        format!("hp={hp}")
    );
    assert_eq!(
        vm.execute_typed::<_, i32>(&program, "hit", (player, 1), &Default::default())
            .unwrap(),
        hp - 1
    );
}

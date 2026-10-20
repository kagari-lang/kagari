//! Deterministic interruption inside external graph admission.
use super::*;
use crate::artifact::{ArtifactValidationError, KbcArtifact};
use kagari_common::identity::{ModuleIdentity, map::DefinitionContext};
use std::cell::Cell;

thread_local! {
    static INTERRUPT: Cell<(usize, Option<usize>)> = const { Cell::new((0, None)) };
}

pub(super) fn after_module(cancel: &CancellationToken) {
    INTERRUPT.with(|state| {
        let (count, stop) = state.get();
        state.set((count + 1, stop));
        if stop == Some(count + 1) {
            cancel.cancel();
        }
    });
}

#[test]
fn external_artifact_cancellation_during_graph_validation_never_returns_a_seal() {
    let context = DefinitionContext::new().unwrap();
    let modules = (0..32)
        .map(|index| BytecodeModule {
            identity: ModuleIdentity::single_file(format!("module{index}.kgr")),
            dependencies: if index == 0 {
                (1..32).map(ModuleRef::new).collect()
            } else {
                vec![]
            },
            ..Default::default()
        })
        .collect();
    let artifact = KbcArtifact::from_program(
        BytecodeProgram {
            root: ModuleRef::new(0),
            modules,
            exports: Default::default(),
        },
        Default::default(),
        &context,
        &Default::default(),
    )
    .unwrap();
    let cancel = CancellationToken::default();
    INTERRUPT.set((0, Some(16)));
    let result = artifact.into_verified(&Default::default(), &context, &cancel);
    assert!(matches!(
        result,
        Err(ArtifactValidationError::Bytecode(
            BytecodeVerificationError::Cancelled
        ))
    ));
    assert_eq!(INTERRUPT.get().0, 16);
    INTERRUPT.set((0, None));
}

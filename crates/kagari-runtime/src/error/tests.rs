use super::{RuntimeError, RuntimeErrorKind};
use crate::{
    error::{
        context::ErrorLocation,
        reason::{ErrorReason, ReflectiveFailure},
    },
    error_trace::{ErrorFrame, ErrorTrace},
    task::{TaskFailureOrigin, TaskId, store::Identity},
};
use kagari_bytecode::{artifact::ArtifactFingerprint, module::CallableTarget, program::ModuleRef};
use kagari_contract::ids::FunctionRef;
use kagari_types::{arithmetic::ArithmeticError, scalar::error::ScalarError};
use std::{error::Error, sync::Arc};

#[test]
fn runtime_errors_expose_stable_codes() {
    let error = RuntimeError::execution_phase_violation("external candidate access");

    assert_eq!(
        RuntimeErrorKind::ExecutionPhaseViolation.code(),
        "KG_RUNTIME_EXECUTION_PHASE_VIOLATION"
    );
    assert_eq!(error.code(), "KG_RUNTIME_EXECUTION_PHASE_VIOLATION");
    assert_eq!(error.kind(), RuntimeErrorKind::ExecutionPhaseViolation);
    assert_eq!(
        RuntimeErrorKind::IndexOutOfBounds.code(),
        "KG_RUNTIME_INDEX_OUT_OF_BOUNDS"
    );
}

#[test]
fn typed_reasons_and_owned_positions_retain_the_first_execution_origin() {
    let trace = Arc::new(ErrorTrace {
        frames: vec![ErrorFrame {
            epoch: 1,
            code_fingerprint: ArtifactFingerprint(7),
            module: ModuleRef::new(0),
            target: CallableTarget::Script(FunctionRef::new(0)),
            function_name: "origin".into(),
            source_uri: "mem://origin".into(),
            instruction_offset: 3,
            tail_calls_elided: false,
            semantic_origin: None,
            source_span: None,
            line: None,
            column: None,
        }],
        ..Default::default()
    });
    let origin = TaskFailureOrigin {
        task: TaskId(Identity {
            owner: 1,
            slot: 0,
            generation: 0,
        }),
        cancellation: None,
    };
    let reason = ScalarError::from(ArithmeticError::DivisionByZero);
    let error = RuntimeError::from(reason)
        .with_trace(trace.clone())
        .with_task_origin(origin)
        .at(ErrorLocation::Field("hp".into()))
        .at(ErrorLocation::Variant("Some".into()))
        .at(ErrorLocation::Element(3));
    assert_eq!(error.reason(), &ErrorReason::Scalar(reason));
    assert_eq!(error.code(), "KG_RUNTIME_SCRIPT_TRAP");
    assert_eq!(
        error.to_string(),
        "element 3: variant Some: field hp: integer division by zero"
    );
    assert_eq!(
        error.locations(),
        &[
            ErrorLocation::Field("hp".into()),
            ErrorLocation::Variant("Some".into()),
            ErrorLocation::Element(3)
        ]
    );

    let copied = error
        .clone()
        .with_trace(Arc::new(ErrorTrace::default()))
        .with_task_origin(TaskFailureOrigin {
            task: TaskId(Identity {
                owner: 2,
                slot: 0,
                generation: 0,
            }),
            cancellation: None,
        });
    assert_eq!(copied, error);
    assert!(Arc::ptr_eq(copied.trace().unwrap(), &trace));
    assert_eq!(copied.task_origin(), Some(origin));

    let write = copied.into_reflective_write();
    assert_eq!(write.kind(), RuntimeErrorKind::InvalidReflectiveWrite);
    assert_eq!(
        write.to_string(),
        format!("invalid reflective write: {error}")
    );
    assert!(Arc::ptr_eq(write.trace().unwrap(), &trace));
    assert_eq!(write.task_origin(), Some(origin));
    let ErrorReason::InvalidReflectiveWrite(ReflectiveFailure::Cause(source)) = write.reason()
    else {
        panic!("typed reflective source")
    };
    assert_eq!(source.as_ref(), &error);
    assert!(
        write
            .source()
            .unwrap()
            .source()
            .unwrap()
            .is::<ReflectiveFailure>()
    );
}

#[test]
fn retained_error_storage_counts_owned_capacity_but_excludes_shared_trace() {
    let reason = ScalarError::from(ArithmeticError::Overflow);
    assert_eq!(RuntimeError::from(reason).retained_bytes(), 0);
    let positioned = |capacity| {
        let mut label = String::with_capacity(capacity);
        label.push_str("payload");
        RuntimeError::from(reason).at(ErrorLocation::Field(label))
    };
    let small = positioned(256);
    let large = positioned(8192);
    assert!(
        small.retained_bytes() > 256,
        "context and location backing are owned too"
    );
    assert_eq!(large.retained_bytes() - small.retained_bytes(), 8192 - 256);
    let bytes = large.retained_bytes();
    let shared = Arc::new(ErrorTrace {
        frames: Vec::with_capacity(128),
        ..Default::default()
    });
    let traced = large.with_trace(shared.clone());
    assert_eq!(traced.retained_bytes(), bytes);
    assert!(Arc::ptr_eq(traced.trace().unwrap(), &shared));
}

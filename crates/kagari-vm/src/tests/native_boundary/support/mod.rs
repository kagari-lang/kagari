use kagari_bytecode::{artifact::KbcArtifact, program::BytecodeProgram};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind, DefinitionPath, map::DefinitionContext, metadata::DefinitionMetadata,
    },
};
use kagari_runtime::{
    Runtime,
    error::RuntimeError,
    frame::ExecutionFrame,
    native::module::NativeModule,
    session::{ExecutionEvent, ExecutionObserver},
};
use std::cell::Cell;

/// Test-only interruption at an observed execution boundary; not an engine budget.
#[derive(Debug)]
pub struct CancelAt {
    pub seen: Cell<usize>,
    pub at: usize,
    pub token: CancellationToken,
}

impl ExecutionObserver for CancelAt {
    fn observe(
        &mut self,
        runtime: &Runtime,
        event: ExecutionEvent,
        _: &[ExecutionFrame],
    ) -> Result<(), RuntimeError> {
        if event == ExecutionEvent::BeforeInstruction {
            let seen = self.seen.get();
            self.seen.set(seen + 1);
            if seen == self.at {
                self.token.cancel();
            }
        }
        runtime.resources().poll_execution()
    }
}

/// Encode, decode and adopt an executable fixture without discarding its identity owner.
pub fn round_trip_program(
    program: DefinitionMetadata<BytecodeProgram>,
) -> DefinitionMetadata<BytecodeProgram> {
    let context = DefinitionContext::new().unwrap();
    let program = program
        .import_into(&context, &Default::default())
        .unwrap()
        .into_records();
    let artifact =
        KbcArtifact::from_program(program, Default::default(), &context, &Default::default())
            .unwrap();
    let bytes = artifact.to_bytes(&context.snapshot()).unwrap();
    let receiver = DefinitionContext::new().unwrap();
    let decoded = KbcArtifact::from_bytes(&bytes)
        .unwrap()
        .import_into(&receiver, &Default::default())
        .unwrap()
        .into_records();
    decoded
        .validate_for_loader(&Default::default(), &receiver, &Default::default())
        .unwrap();
    DefinitionMetadata::checked(receiver.snapshot(), decoded.program, &Default::default()).unwrap()
}

/// Materialize one exact external declaration query from an independently registered module.
pub fn declaration_query(
    module: &NativeModule,
    kind: DefinitionKind,
    name: &str,
) -> DefinitionPath {
    let declaration = module.to_declaration();
    let id = declaration
        .records()
        .definition(kind, name, declaration.definitions())
        .unwrap();
    declaration.definitions().resolve(id).unwrap().to_path()
}

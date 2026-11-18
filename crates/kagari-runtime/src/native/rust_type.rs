//! Runtime-bound Rust target identity for Opaque payloads.
use crate::{
    Runtime,
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    module::{
        ExecutableModule, ModuleEpochRetention, managed::source::RevisionSource,
        retention::ProgramLease,
    },
    native::binding::NativeResult,
};
use kagari_types::{declaration::native::NativeStorageLayout, ty::Ty};
use std::marker::PhantomData;

/// A checked Rust representation of one applied nominal type and generation.
#[derive(Debug)]
pub(super) struct RustType<T> {
    argument: TypeArgument,
    owner: ExecutableModule,
    program: ProgramLease,
    mapping: PhantomData<fn() -> T>,
}

impl<T> RustType<T> {
    pub(super) fn type_argument(&self) -> &TypeArgument {
        &self.argument
    }

    pub(super) fn owner(&self) -> &ExecutableModule {
        &self.owner
    }

    pub(super) fn program(&self) -> ProgramLease {
        self.program.clone()
    }
}

impl<T: Send + 'static> RustType<T> {
    pub(crate) fn prepare(
        runtime: &mut Runtime,
        owner: &ExecutableModule,
        argument: TypeArgument,
        origin: RevisionSource<'_>,
    ) -> NativeResult<Self> {
        runtime.resources().ensure_execution_allowed()?;
        runtime.validate_executable_module(owner)?;
        check_mapping::<T>(runtime, owner, &argument)?;
        let program = runtime
            .retain_program(owner, ModuleEpochRetention::RuntimeValue, origin)
            .ok_or_else(invalid)?;
        Ok(Self {
            argument,
            owner: owner.clone(),
            program,
            mapping: PhantomData,
        })
    }
}

pub(crate) fn check_mapping<T: Send + 'static>(
    runtime: &Runtime,
    owner: &ExecutableModule,
    expected: &TypeArgument,
) -> NativeResult<()> {
    runtime.resources().ensure_execution_allowed()?;
    runtime.validate_executable_module(owner)?;
    expected.validate(runtime)?;
    match expected.ty() {
        Ty::NativeObject(nominal) => {
            if runtime
                .native_entries
                .storage
                .get_id(nominal.declaration)
                .is_none_or(|storage| {
                    storage.layout() != NativeStorageLayout::Opaque
                        || !storage.accepts_payload::<T>()
                })
            {
                return Err(invalid());
            }
        }
        _ => return Err(invalid()),
    }
    Ok(())
}

fn invalid() -> RuntimeError {
    RuntimeError::module_validation(
        "Rust payload requires its installed Opaque representation and application",
    )
}

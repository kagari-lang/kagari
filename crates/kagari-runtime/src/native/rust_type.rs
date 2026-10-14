//! Runtime-bound Rust target identity, independent of owned versus external storage.
use crate::{
    Runtime,
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    module::{LoadedModule, ModuleEpochRetention, retention::ProgramLease},
    native::{
        binding::NativeResult,
        conversion::{KagariType, context::ConversionContext},
    },
};
use kagari_types::{declaration::native::NativeStorageLayout, ty::Ty};
use std::{marker::PhantomData, sync::Arc};

#[derive(Debug)]
struct TypeRecord {
    argument: TypeArgument,
    owner: LoadedModule,
    program: ProgramLease,
}

/// A checked Rust representation of one applied nominal target and generation.
/// It contains no object or address. External scopes reserve the original Rust
/// value through this descriptor; ordinary GC structs remain owned script data.
#[derive(Debug)]
pub struct RustType<T> {
    record: Arc<TypeRecord>,
    mapping: PhantomData<fn() -> T>,
}

impl<T> Clone for RustType<T> {
    fn clone(&self) -> Self {
        Self {
            record: Arc::clone(&self.record),
            mapping: PhantomData,
        }
    }
}

impl<T> RustType<T> {
    pub fn type_argument(&self) -> &TypeArgument {
        &self.record.argument
    }

    pub fn owner(&self) -> &LoadedModule {
        &self.record.owner
    }

    pub(crate) fn program(&self) -> ProgramLease {
        self.record.program.clone()
    }
}

impl<T: Send + 'static> RustType<T> {
    pub(crate) fn prepare(
        runtime: &Runtime,
        owner: &LoadedModule,
        argument: TypeArgument,
    ) -> NativeResult<Self> {
        runtime.gc().ensure_no_native_borrow()?;
        runtime.resources().ensure_execution_allowed()?;
        runtime.validate_loaded_module(owner)?;
        check_mapping::<T>(runtime, owner, &argument)?;
        let program = runtime
            .retain_program(owner, ModuleEpochRetention::RuntimeValue)
            .ok_or_else(invalid)?;
        Ok(Self {
            record: Arc::new(TypeRecord {
                argument,
                owner: owner.clone(),
                program,
            }),
            mapping: PhantomData,
        })
    }
}

impl Runtime {
    /// Bind an explicitly registered ordinary Rust struct representation. Native
    /// payload constructors already supply this same descriptor through NativeType.
    /// Custom conversion schemas do not prove Rust field layout or projectors.
    pub fn bind_rust_type<T: KagariType + Send + 'static>(
        &self,
        owner: &LoadedModule,
    ) -> NativeResult<RustType<T>> {
        let cx = ConversionContext::new(self, owner)?;
        let argument = cx.type_for::<T>()?;
        RustType::prepare(self, owner, argument)
    }
}

pub(crate) fn check_mapping<T: Send + 'static>(
    runtime: &Runtime,
    owner: &LoadedModule,
    expected: &TypeArgument,
) -> NativeResult<()> {
    runtime.gc().ensure_no_native_borrow()?;
    runtime.resources().ensure_execution_allowed()?;
    runtime.validate_loaded_module(owner)?;
    expected.validate(runtime)?;
    match expected.ty() {
        Ty::Struct(_) => {
            let ty = runtime.native_entries.catalog.rust_type::<T>()?;
            let actual = runtime.native_type_argument(owner, &ty)?;
            if !expected.view(owner).compatible(actual.view(owner)) {
                return Err(invalid());
            }
        }
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
        "Rust host target requires its installed struct or Opaque representation and application",
    )
}

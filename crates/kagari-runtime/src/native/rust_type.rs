//! Runtime-bound Rust target identity, independent of owned versus external storage.
use crate::{
    Runtime,
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    module::{ExecutableModule, Module, ModuleEpochRetention, retention::ProgramLease},
    native::{
        binding::NativeResult,
        catalog::DeclarationCatalog,
        conversion::{
            KagariType,
            context::{ConversionContext, ConversionRetention},
        },
        projection::GeneratedField,
    },
};
use kagari_common::identity::{DefinitionKind, table::DefinitionId};
use kagari_types::{declaration::native::NativeStorageLayout, ty::Ty};
use std::{any::TypeId as RustTypeId, marker::PhantomData, sync::Arc};

#[derive(Debug)]
struct TypeRecord {
    argument: TypeArgument,
    owner: ExecutableModule,
    program: ProgramLease,
    catalog: DeclarationCatalog,
}

/// One generated direct field in an exact applied Rust target and generation.
/// Resolving this descriptor grants no loan and caches no field address.
#[derive(Debug)]
pub struct RustField<T, F> {
    pub(crate) parent: RustType<T>,
    pub(crate) declaration: DefinitionId,
    pub(crate) projector: GeneratedField,
    pub(crate) target: TypeArgument,
    mapping: PhantomData<fn() -> F>,
}

impl<T, F> Clone for RustField<T, F> {
    fn clone(&self) -> Self {
        Self {
            parent: self.parent.clone(),
            declaration: self.declaration,
            projector: self.projector.clone(),
            target: self.target.clone(),
            mapping: PhantomData,
        }
    }
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

    pub fn owner(&self) -> &ExecutableModule {
        &self.record.owner
    }

    pub(crate) fn program(&self) -> ProgramLease {
        self.record.program.clone()
    }

    pub(crate) fn catalog(&self) -> &DeclarationCatalog {
        &self.record.catalog
    }
}

impl<T: Send + 'static> RustType<T> {
    pub(crate) fn prepare(
        runtime: &Runtime,
        owner: &ExecutableModule,
        argument: TypeArgument,
        authority: Option<ConversionRetention<'_>>,
    ) -> NativeResult<Self> {
        runtime.gc().ensure_no_storage_borrow()?;
        runtime.resources().ensure_execution_allowed()?;
        runtime.validate_executable_module(owner)?;
        check_mapping::<T>(runtime, owner, &argument)?;
        let program = match authority {
            Some(authority) => authority.retain(runtime, owner, ModuleEpochRetention::RuntimeValue),
            None => runtime.retain_program(owner, ModuleEpochRetention::RuntimeValue),
        }
        .ok_or_else(invalid)?;
        Ok(Self {
            record: Arc::new(TypeRecord {
                argument,
                owner: owner.clone(),
                program,
                catalog: runtime.native_entries.catalog.clone(),
            }),
            mapping: PhantomData,
        })
    }
}

impl Runtime {
    /// Resolve a generated public inline field. DTO and Serde schemas without
    /// dedicated layout proofs reject, as do mismatched Rust field representations.
    pub fn bind_rust_field<T: Send + 'static, F: KagariType + Send + 'static>(
        &self,
        parent: &RustType<T>,
        name: &str,
    ) -> NativeResult<RustField<T, F>> {
        self.validate_executable_module(parent.owner())?;
        parent.type_argument().validate(self)?;
        let Ty::Struct(nominal) = parent.type_argument().ty() else {
            return Err(invalid());
        };
        let fields = parent
            .record
            .catalog
            .rust_types
            .get(&RustTypeId::of::<T>())
            .and_then(|mapping| mapping.fields.as_ref())
            .ok_or_else(|| {
                RuntimeError::module_validation("Rust target has no generated inline field layout")
            })?;
        let projector = fields
            .iter()
            .find(|field| field.name == name && field.target == RustTypeId::of::<F>())
            .cloned()
            .ok_or_else(invalid)?;
        let declaration = self
            .definition_context()
            .snapshot()
            .lookup_child(nominal.declaration, DefinitionKind::Field, name, 0)
            .ok_or_else(invalid)?;
        let ty = (projector.target_type)(&parent.record.catalog)?;
        let target = self.native_type_argument(parent.owner(), &ty)?;
        let cx = ConversionContext::new(self, parent.owner())?;
        F::check_type(&cx, &target)?;
        Ok(RustField {
            parent: parent.clone(),
            declaration,
            projector,
            target,
            mapping: PhantomData,
        })
    }

    /// Bind an explicitly registered ordinary Rust struct representation. Native
    /// payload constructors already supply this same descriptor through NativeType.
    /// Custom conversion schemas do not prove Rust field layout or projectors.
    pub fn bind_rust_type<T: KagariType + Send + 'static>(
        &self,
        owner: &Module,
    ) -> NativeResult<RustType<T>> {
        let snapshot = self.capture_module(owner)?;
        let owner = &snapshot.root;
        let cx = ConversionContext::new(self, owner)?;
        let argument = cx.type_for::<T>()?;
        RustType::prepare(self, owner, argument, Some(cx.retention()))
    }
}

pub(crate) fn check_mapping<T: Send + 'static>(
    runtime: &Runtime,
    owner: &ExecutableModule,
    expected: &TypeArgument,
) -> NativeResult<()> {
    runtime.gc().ensure_no_storage_borrow()?;
    runtime.resources().ensure_execution_allowed()?;
    runtime.validate_executable_module(owner)?;
    expected.validate(runtime)?;
    match expected.ty() {
        Ty::Struct(_) => {
            let ty = runtime.native_entries.catalog.rust_type::<T>()?;
            let actual = runtime.native_type_argument(owner, &ty)?;
            if !expected.view(owner).compatible(actual.view(owner)) {
                return Err(invalid());
            }
        }
        Ty::NativeObject(nominal) => check_opaque_mapping::<T>(runtime, nominal.declaration)?,
        _ => return Err(invalid()),
    }
    Ok(())
}

/// Check installed storage after the caller has validated the argument's scope.
/// Dynamic execution/owner admission stays with the enclosing operation.
pub(in crate::native) fn check_opaque_mapping<T: Send + 'static>(
    runtime: &Runtime,
    declaration: DefinitionId,
) -> NativeResult<()> {
    if runtime
        .native_entries
        .storage
        .get_id(declaration)
        .is_none_or(|storage| {
            storage.layout() != NativeStorageLayout::Opaque || !storage.accepts_payload::<T>()
        })
    {
        return Err(invalid());
    }
    Ok(())
}

fn invalid() -> RuntimeError {
    RuntimeError::module_validation(
        "Rust host target requires its installed struct or Opaque representation and application",
    )
}

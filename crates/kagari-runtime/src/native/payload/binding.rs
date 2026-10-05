use crate::{
    Runtime,
    error::RuntimeError,
    frame::types::{arguments::TypeArgument, bindings::TypeBindings},
    module::{LoadedModule, ModuleEpochRetention},
    native::{
        binding::NativeResult,
        payload::{NativeType, TypeRecord, managed::AppliedSchema},
        storage::NativePayload,
        types::TypeRef,
    },
};
use kagari_common::identity::DefinitionPath;
use kagari_types::{
    declaration::{TypeDefKind, native::NativeStorageLayout},
    ty::{NominalTy, Ty},
};
use std::{marker::PhantomData, sync::Arc};

impl Runtime {
    pub fn bind_native_type<T: NativePayload>(
        &self,
        owner: &LoadedModule,
        declaration: &TypeRef,
        arguments: &[TypeArgument],
    ) -> NativeResult<NativeType<T>> {
        self.bind_native_type_declaration(owner, declaration.id(), arguments)
    }

    pub fn bind_native_type_declaration<T: NativePayload>(
        &self,
        owner: &LoadedModule,
        declaration: &DefinitionPath,
        arguments: &[TypeArgument],
    ) -> NativeResult<NativeType<T>> {
        self.gc().ensure_no_native_borrow()?;
        self.resources().ensure_execution_allowed()?;
        self.validate_loaded_module(owner)?;
        let declared = self
            .native_entries
            .catalog
            .types
            .get(declaration)
            .ok_or_else(invalid)?;
        if declared.kind != TypeDefKind::NativeStorage(NativeStorageLayout::Opaque)
            || !declared.bounds.is_empty()
            || declared.generic_params.len() != arguments.len()
        {
            return Err(invalid());
        }
        for argument in arguments {
            argument.validate(self)?;
        }
        let id = self
            .definition_context()
            .snapshot()
            .lookup(declaration)
            .ok_or_else(invalid)?;
        let environment = Arc::new(TypeBindings::new(
            self.definition_context(),
            declared.generic_params.clone(),
            arguments.to_vec(),
        )?);
        let expression = Ty::NativeObject(NominalTy {
            declaration: id,
            arguments: declared
                .generic_params
                .iter()
                .map(|parameter| parameter.as_type())
                .collect(),
            associated_types: Default::default(),
        });
        let argument = self
            .type_arguments(owner, Some(environment), &[expression])?
            .pop()
            .ok_or_else(invalid)?;
        self.prepare_native_type(owner, argument)
    }

    pub(crate) fn prepare_native_type<T: NativePayload>(
        &self,
        owner: &LoadedModule,
        argument: TypeArgument,
    ) -> NativeResult<NativeType<T>> {
        self.gc().ensure_no_native_borrow()?;
        self.resources().ensure_execution_allowed()?;
        self.validate_loaded_module(owner)?;
        argument.validate(self)?;
        let Ty::NativeObject(nominal) = argument.ty() else {
            return Err(invalid());
        };
        let storage = self
            .native_entries
            .storage
            .get_id(nominal.declaration)
            .ok_or_else(invalid)?;
        if storage.layout() != NativeStorageLayout::Opaque || !storage.accepts_payload::<T>() {
            return Err(invalid());
        }
        let program = self
            .retain_program(owner, ModuleEpochRetention::RuntimeValue)
            .ok_or_else(invalid)?;
        let managed = storage
            .managed_schema()
            .map(|schema| {
                AppliedSchema::prepare(self, owner, &argument, schema.clone()).map(Arc::new)
            })
            .transpose()?;
        Ok(NativeType {
            record: Arc::new(TypeRecord {
                argument,
                owner: owner.clone(),
                storage: storage.clone(),
                managed,
                _program: program,
            }),
            mapping: PhantomData,
        })
    }
}

fn invalid() -> RuntimeError {
    RuntimeError::module_validation(
        "native object requires its installed payload type and application",
    )
}

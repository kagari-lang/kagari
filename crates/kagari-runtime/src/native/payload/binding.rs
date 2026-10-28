use crate::{
    Runtime,
    error::RuntimeError,
    frame::types::{arguments::TypeArgument, bindings::TypeBindings, compatibility::TypeView},
    module::{ExecutableModule, Module},
    native::{
        binding::{LinkedNativeFunction, NativeResult},
        conversion::context::ConversionRetention,
        payload::{DataRecord, NativeType, managed::AppliedSchema},
        rust_type::{RustType, check_mapping},
        storage::NativePayload,
        types::TypeRef,
    },
    value::Value,
};

use kagari_common::identity::table::{DefinitionId, DefinitionTable};
use kagari_types::{
    declaration::{TypeDefKind, native::NativeStorageLayout},
    ty::{NominalTy, Ty},
};
use std::sync::Arc;

impl Runtime {
    pub fn bind_native_type<T: NativePayload>(
        &self,
        owner: &Module,
        declaration: &TypeRef,
        arguments: &[TypeArgument],
    ) -> NativeResult<NativeType<T>> {
        self.bind_native_type_declaration(
            owner,
            declaration.id(),
            declaration.definitions(),
            arguments,
        )
    }

    pub fn bind_native_type_declaration<T: NativePayload>(
        &self,
        owner: &Module,
        declaration: DefinitionId,
        definitions: &DefinitionTable,
        arguments: &[TypeArgument],
    ) -> NativeResult<NativeType<T>> {
        self.gc().ensure_no_native_borrow()?;
        let snapshot = self.capture_module(owner)?;
        let owner = &snapshot.root;
        let argument = self.apply_native_type(owner, declaration, definitions, arguments)?;
        self.prepare_native_type(owner, argument, None)
    }

    pub(crate) fn apply_native_type(
        &self,
        owner: &ExecutableModule,
        declaration: DefinitionId,
        definitions: &DefinitionTable,
        arguments: &[TypeArgument],
    ) -> NativeResult<TypeArgument> {
        self.gc().ensure_no_native_borrow()?;
        self.validate_executable_module(owner)?;
        let id = self
            .definition_context()
            .import(definitions, [declaration])
            .and_then(|remap| remap.map(declaration))
            .map_err(|error| RuntimeError::module_validation(error.to_string()))?;
        let declared = self
            .native_entries
            .catalog
            .types
            .get_id(id)
            .ok_or_else(invalid)?;
        if declared.kind != TypeDefKind::NativeStorage(NativeStorageLayout::Opaque)
            || declared.generic_params.len() != arguments.len()
        {
            return Err(invalid());
        }
        for argument in arguments {
            argument.validate(self)?;
        }
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
        Ok(argument)
    }

    pub(crate) fn prepare_native_type<T: Send + 'static>(
        &self,
        owner: &ExecutableModule,
        argument: TypeArgument,
        authority: Option<ConversionRetention<'_>>,
    ) -> NativeResult<NativeType<T>> {
        self.gc().ensure_no_native_borrow()?;
        self.resources().ensure_execution_allowed()?;
        self.validate_executable_module(owner)?;
        argument.validate(self)?;
        let Ty::NativeObject(nominal) = argument.ty() else {
            return Err(invalid());
        };
        let declared = self
            .native_entries
            .catalog
            .types
            .get_id(nominal.declaration)
            .ok_or_else(invalid)?;
        let carrier = if declared.bounds.is_empty() {
            owner.clone()
        } else {
            self.native_application_owner(owner, &argument)?
        };
        self.prepare_admitted_native_type(&carrier, argument, authority)
    }

    /// An active native call can instantiate only its exact import's result witness.
    /// The linked callback signature already uses an adapter's concrete receiver.
    pub(crate) fn prepare_native_result_type<T: Send + 'static>(
        &self,
        owner: &ExecutableModule,
        function: &LinkedNativeFunction,
        authority: ConversionRetention<'_>,
    ) -> NativeResult<NativeType<T>> {
        let argument = self.native_payload_result_argument(owner, function)?;
        self.prepare_admitted_native_type(owner, argument, Some(authority))
    }

    /// Shared admission for typed opaque creation and the low-level payload adapter.
    /// Legacy nonopaque storage keeps its unbounded declared-result contract.
    pub(crate) fn native_payload_result_argument(
        &self,
        owner: &ExecutableModule,
        function: &LinkedNativeFunction,
    ) -> NativeResult<TypeArgument> {
        self.gc().ensure_no_native_borrow()?;
        self.resources().ensure_execution_allowed()?;
        self.validate_executable_module(owner)?;
        let import = owner
            .bytecode
            .native_imports
            .get(function.import.index())
            .ok_or_else(invalid)?;
        if import.instance.declaration != function.declaration {
            return Err(invalid());
        }
        let environment = match (&import.generic, &function.environment) {
            (None, None) => None,
            (Some(body), Some(environment))
                if self.gc().environment(environment.id).is_some()
                    && environment.types.matches(body) =>
            {
                Some(environment.types.clone())
            }
            _ => return Err(invalid()),
        };
        let result = import
            .result_adapter
            .as_ref()
            .map_or(&import.signature.result, |adapter| &adapter.receiver);
        let Ty::NativeObject(nominal) = result else {
            return Err(invalid());
        };
        let declared = self
            .native_entries
            .catalog
            .types
            .get_id(nominal.declaration)
            .ok_or_else(invalid)?;
        if declared.kind != TypeDefKind::NativeStorage(NativeStorageLayout::Opaque)
            && !declared.bounds.is_empty()
        {
            return Err(invalid());
        }
        // Linking checked this exact carrier/import's result witness, including
        // its binder. Signature preparation/application owns the immutable closed
        // facts. Compare their scoped meaning without rebuilding a TypeArgument;
        // live owner/environment admission above is still required on every call.
        let prepared = &function.type_signature()?.result;
        prepared.validate(self)?;
        if !TypeView::new(result, owner, environment.as_deref()).compatible(prepared.view(owner)) {
            return Err(invalid());
        }
        Ok(prepared.clone())
    }

    /// A validated object's heap admission is distinct from host occurrence discovery.
    /// This cannot admit a bare TypeArgument: its value, scope, generation and payload
    /// representation must all agree before the descriptor can escape.
    pub(super) fn native_data_from_object<T: Send + 'static>(
        &self,
        owner: &ExecutableModule,
        argument: TypeArgument,
        value: &Value,
    ) -> NativeResult<Arc<DataRecord>> {
        self.gc().ensure_no_native_borrow()?;
        self.resources().ensure_execution_allowed()?;
        self.validate_executable_module(owner)?;
        argument.validate(self)?;
        let Value::GcHandle(id) = value else {
            return Err(invalid());
        };
        if !argument.matches(self, value, owner) {
            return Err(invalid());
        }
        self.gc().check_native_payload::<T>(*id, false)?;
        self.prepare_native_data::<T>(owner, argument)
    }

    fn prepare_admitted_native_type<T: Send + 'static>(
        &self,
        owner: &ExecutableModule,
        argument: TypeArgument,
        authority: Option<ConversionRetention<'_>>,
    ) -> NativeResult<NativeType<T>> {
        let data = self.prepare_native_data::<T>(owner, argument)?;
        self.prepare_native_data_type(data, authority)
    }

    pub(super) fn prepare_native_data_type<T: Send + 'static>(
        &self,
        data: Arc<DataRecord>,
        authority: Option<ConversionRetention<'_>>,
    ) -> NativeResult<NativeType<T>> {
        let rust_type = RustType::prepare(self, &data.owner, data.argument.clone(), authority)?;
        Ok(NativeType {
            record: data,
            rust_type,
        })
    }

    fn prepare_native_data<T: Send + 'static>(
        &self,
        owner: &ExecutableModule,
        argument: TypeArgument,
    ) -> NativeResult<Arc<DataRecord>> {
        check_mapping::<T>(self, owner, &argument)?;
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
        let managed = storage
            .managed_schema()
            .map(|schema| {
                AppliedSchema::prepare(self, owner, &argument, schema.clone()).map(Arc::new)
            })
            .transpose()?;
        Ok(Arc::new(DataRecord {
            argument,
            owner: owner.clone(),
            storage: storage.clone(),
            managed,
        }))
    }

    /// Host admission uses sealed occurrences, never a runtime trait search. An
    /// arbitrary type argument cannot instantiate an open carrier's assumptions.
    fn native_application_owner(
        &self,
        owner: &ExecutableModule,
        argument: &TypeArgument,
    ) -> NativeResult<ExecutableModule> {
        let Ty::NativeObject(nominal) = argument.ty() else {
            return Err(invalid());
        };
        let arguments = (0..nominal.arguments.len())
            .map(|index| argument.parameter(self, owner, index))
            .collect::<NativeResult<Vec<_>>>()?;
        for witness in owner.verified_program().native_applications() {
            if !witness.is_closed() || witness.nominal() != nominal {
                continue;
            }
            let carrier = owner.member(witness.carrier()).ok_or_else(invalid)?;
            if arguments
                .iter()
                .zip(&witness.nominal().arguments)
                .all(|(argument, ty)| {
                    argument
                        .view(owner)
                        .compatible(TypeView::new(ty, &carrier, None))
                })
                && argument
                    .view(owner)
                    .compatible(TypeView::new(argument.ty(), &carrier, None))
            {
                return Ok(carrier);
            }
        }
        Err(RuntimeError::module_validation(
            "bounded native application has no closed verified occurrence in the supplying type scope",
        ))
    }
}

fn invalid() -> RuntimeError {
    RuntimeError::module_validation(
        "native object requires its installed payload type and application",
    )
}

use crate::{
    Runtime,
    error::RuntimeError,
    frame::types::{arguments::TypeArgument, bindings::TypeBindings},
    module::LoadedModule,
    native::{
        binding::NativeResult,
        conversion::{KagariType, context::ConversionContext},
        payload::{
            NativeType,
            data::NativeData,
            managed::{
                AppliedSchema, BoundField, Field, FieldType, Managed, ManagedSchema, invalid,
            },
        },
        storage::NativeStorage,
        storage_type::StorageType,
    },
};
use kagari_common::identity::{
    mapping::{DefinitionMapper, DefinitionMappingError, DefinitionRecord},
    reference::DefinitionReference,
};
use kagari_types::ty::Ty;
use std::{marker::PhantomData, ptr, sync::Arc};

impl AppliedSchema {
    pub(crate) fn registered_in(&self, storage: &NativeStorage) -> bool {
        storage
            .managed_schema()
            .is_some_and(|schema| Arc::ptr_eq(schema, &self.declaration))
    }

    pub(crate) fn prepare(
        runtime: &Runtime,
        owner: &LoadedModule,
        argument: &TypeArgument,
        declaration: Arc<ManagedSchema>,
    ) -> NativeResult<Self> {
        let Ty::NativeObject(nominal) = argument.ty() else {
            return Err(invalid());
        };
        let declared = runtime
            .native_entries
            .catalog
            .types
            .get_id(nominal.declaration)
            .ok_or_else(invalid)?;
        let arguments = (0..declared.generic_params.len())
            .map(|index| argument.parameter(runtime, owner, index))
            .collect::<NativeResult<Vec<_>>>()?;
        let environment = Arc::new(TypeBindings::new(
            runtime.definition_context(),
            declared.generic_params.clone(),
            arguments,
        )?);
        let definitions = runtime.definition_context().snapshot();
        let mut fields = Vec::new();
        fields
            .try_reserve_exact(declaration.fields.len())
            .map_err(|_| RuntimeError::resource_limit("managed field applications"))?;
        for field in &declaration.fields {
            let ty = field
                .ty
                .abi()
                .map_identities(&mut DefinitionMapper::new(
                    &mut |id| {
                        id.resolve(&definitions)
                            .map_err(DefinitionMappingError::from)
                    },
                    &Default::default(),
                ))
                .map_err(|error| RuntimeError::module_validation(error.to_string()))?;
            let argument = runtime
                .type_arguments(owner, Some(environment.clone()), &[ty])?
                .pop()
                .ok_or_else(invalid)?;
            let contract = StorageType::prepare_scoped(argument.clone(), owner)?;
            fields.push(FieldType { argument, contract });
        }
        Ok(Self {
            declaration,
            argument: argument.clone(),
            owner: owner.clone(),
            fields,
        })
    }

    pub(crate) fn matches(&self, other: &Self) -> bool {
        if ptr::eq(self, other) {
            return true;
        }
        Arc::ptr_eq(&self.declaration.identity, &other.declaration.identity)
            && self
                .argument
                .view(&self.owner)
                .compatible(other.argument.view(&other.owner))
            && self.fields.len() == other.fields.len()
            && self
                .fields
                .iter()
                .zip(&other.fields)
                .all(|(a, b)| a.contract.same_type(&b.contract))
    }
}

impl<T: NativeData> NativeType<Managed<T>> {
    pub fn bind_field<V: KagariType>(
        &self,
        runtime: &Runtime,
        field: &Field<V>,
    ) -> NativeResult<BoundField<T, V>> {
        let cx = ConversionContext::new(runtime, self.owner())?;
        let schema = self.schema()?;
        if !Arc::ptr_eq(&schema.declaration.identity, &field.identity) {
            return Err(invalid());
        }
        let ty = &schema.fields.get(field.slot).ok_or_else(invalid)?.argument;
        cx.check_type::<V>(ty)?;
        Ok(BoundField {
            native_type: self.clone(),
            slot: field.slot,
            mapping: PhantomData,
        })
    }

    pub(crate) fn schema(&self) -> NativeResult<&Arc<AppliedSchema>> {
        self.record.managed.as_ref().ok_or_else(invalid)
    }
}

impl<T: NativeData, V> BoundField<T, V> {
    pub(crate) fn check(&self, native_type: &NativeType<Managed<T>>) -> NativeResult<()> {
        if self.native_type.schema()?.matches(native_type.schema()?) {
            Ok(())
        } else {
            Err(invalid())
        }
    }

    fn field_type(&self) -> &FieldType {
        &self
            .native_type
            .record
            .managed
            .as_ref()
            .expect("bound managed schema")
            .fields[self.slot]
    }

    pub fn type_argument(&self) -> &TypeArgument {
        &self.field_type().argument
    }
}

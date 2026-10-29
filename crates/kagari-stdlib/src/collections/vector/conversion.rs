//! Exact nominal mapping and ordinary generic provider construction.
use crate::{
    collections::vector::{ScriptVec, VecStorage, VecValues},
    identity,
};
use kagari_common::identity::DefinitionKind;
use kagari_runtime::{
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    native::{
        binding::NativeResult,
        catalog::DeclarationCatalog,
        conversion::{FromKagari, IntoKagari, KagariType, context::ConversionContext},
        payload::Opaque,
        schema::TypeRegistration,
        typed::NativeContext,
        types::Type,
        value_handle::ScriptValue,
    },
};
use std::{
    ops::{Deref, DerefMut},
    slice, vec,
};

impl<T: KagariType> KagariType for ScriptVec<T> {
    fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
        catalog
            .type_reference(&identity::type_id("Vec", DefinitionKind::AssociatedType))?
            .apply([T::kagari_type(catalog)?])
    }

    fn register_type(registration: &mut TypeRegistration<'_>) -> NativeResult<Type>
    where
        Self: 'static,
    {
        registration.dependency::<T>()?;
        Self::kagari_type(registration.catalog())
    }

    fn check_type(cx: &ConversionContext<'_>, expected: &TypeArgument) -> NativeResult<()> {
        Opaque::<VecStorage>::check_type(cx, expected)?;
        cx.check_type::<T>(&cx.parameter(expected, 0)?)
    }
}

impl<T: KagariType> FromKagari for ScriptVec<T> {
    const PRESERVES_IDENTITY: bool = true;

    fn from_kagari(
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
        value: &ScriptValue,
    ) -> NativeResult<Self> {
        Self::check_type(cx, expected)?;
        let object = Opaque::<VecStorage>::from_kagari(cx, expected, value)?;
        Self::from_object(&NativeContext::new(cx.runtime(), cx.owner())?, object)
    }
}

impl<T: KagariType> IntoKagari for ScriptVec<T> {
    fn into_kagari(
        self,
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
    ) -> NativeResult<ScriptValue> {
        self.writable()?;
        Self::check_type(cx, expected)?;
        self.object.into_kagari(cx, expected)
    }
}

impl<T: IntoKagari> ScriptVec<T> {
    /// Construct a fresh Vec through its installed ordinary provider.
    pub fn create(cx: &mut NativeContext<'_>, values: Vec<T>) -> NativeResult<Self> {
        let element = cx.conversion().type_for::<T>()?;
        Self::create_with_type(cx, element, values)
    }

    /// Construct using an explicit element scope, including contextual handles.
    pub fn create_with_type(
        cx: &mut NativeContext<'_>,
        element: TypeArgument,
        values: Vec<T>,
    ) -> NativeResult<Self> {
        let declaration = cx
            .conversion()
            .type_reference(&identity::type_id("Vec", DefinitionKind::AssociatedType))?;
        let applied = cx.conversion().apply_type(&declaration, &[element])?;
        cx.conversion().argument_scope(|conversion| {
            let value = conversion.encode_value(&applied, VecValues(values))?;
            conversion.decode_value(&applied, &value)
        })
    }
}

impl<T: KagariType> KagariType for VecValues<T> {
    fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
        ScriptVec::<T>::kagari_type(catalog)
    }

    fn register_type(registration: &mut TypeRegistration<'_>) -> NativeResult<Type>
    where
        Self: 'static,
    {
        ScriptVec::<T>::register_type(registration)
    }

    fn check_type(cx: &ConversionContext<'_>, expected: &TypeArgument) -> NativeResult<()> {
        ScriptVec::<T>::check_type(cx, expected)
    }
}

impl<T: IntoKagari> IntoKagari for VecValues<T> {
    fn into_kagari(
        self,
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
    ) -> NativeResult<ScriptValue> {
        Self::check_type(cx, expected)?;
        cx.check_elements(self.0.len())?;
        let ty = cx.native_type::<VecStorage>(expected)?;
        let mut native = NativeContext::new(cx.runtime(), cx.owner())?;
        let mut payload = VecStorage::empty(&native, &ty)?;
        let element = cx.parameter(expected, 0)?;
        payload
            .values
            .try_reserve(self.0.len())
            .map_err(|_| RuntimeError::resource_limit("Vec initial capacity"))?;
        for value in self.0 {
            // Reuse the caller's conversion frame for depth, cycle and node limits.
            // Its roots protect all initialized entries until payload admission.
            let value = cx.encode_value(&element, value)?;
            // SAFETY: The conversion frame retains each entry even after this
            // local handle drops, until ty.create publishes the complete payload.
            payload
                .values
                .push(unsafe { value.value_transport(cx.runtime()) }?)?;
        }
        ty.create(&mut native, payload, &[])?
            .into_kagari(cx, expected)
    }
}

impl<T: FromKagari> FromKagari for VecValues<T> {
    fn from_kagari(
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
        value: &ScriptValue,
    ) -> NativeResult<Self> {
        let source = ScriptVec::<T>::from_kagari(cx, expected, value)?;
        let native = NativeContext::new(cx.runtime(), cx.owner())?;
        let count = source.len(&native)?;
        cx.check_elements(count)?;
        let mut snapshot = Vec::new();
        snapshot
            .try_reserve_exact(count)
            .map_err(|_| RuntimeError::resource_limit("Vec conversion snapshot"))?;
        // SAFETY: Retain every source edge before any child converter can mutate
        // the source or collect. This owned conversion explicitly needs a snapshot.
        unsafe {
            source.object.read(&native, None, |values, access| {
                for index in 0..count {
                    snapshot.push(access.retain(
                        &values.contract,
                        &values.values.get(index).expect("snapshot index"),
                    )?);
                }
                Ok(())
            })?;
        }
        let element = cx.parameter(expected, 0)?;
        let mut values = Vec::new();
        values
            .try_reserve_exact(count)
            .map_err(|_| RuntimeError::resource_limit("Vec owned conversion"))?;
        for value in &snapshot {
            let raw = value.clone().into_kagari(cx, &element)?;
            values.push(cx.decode_value(&element, &raw)?);
        }
        Ok(Self(values))
    }
}

impl<T> From<Vec<T>> for VecValues<T> {
    fn from(values: Vec<T>) -> Self {
        Self(values)
    }
}

impl<T> FromIterator<T> for VecValues<T> {
    fn from_iter<I: IntoIterator<Item = T>>(values: I) -> Self {
        Self(values.into_iter().collect())
    }
}

impl<T> Deref for VecValues<T> {
    type Target = Vec<T>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> DerefMut for VecValues<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T> IntoIterator for VecValues<T> {
    type Item = T;
    type IntoIter = vec::IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<'a, T> IntoIterator for &'a VecValues<T> {
    type Item = &'a T;
    type IntoIter = slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl<'a, T> IntoIterator for &'a mut VecValues<T> {
    type Item = &'a mut T;
    type IntoIter = slice::IterMut<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter_mut()
    }
}

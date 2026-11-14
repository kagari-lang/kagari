use crate::native::{
    binding::NativeResult,
    conversion::{FromKagari, IntoKagari, context::ConversionContext},
    payload::{
        Opaque,
        data::NativeData,
        managed::{BoundField, Managed},
    },
    typed::NativeContext,
};

impl<T: NativeData> Opaque<Managed<T>> {
    /// Only fixed data is lent; traced fields and their sources remain private.
    pub fn edit_data<R>(
        &self,
        cx: &mut NativeContext<'_>,
        edit: impl for<'data> FnOnce(&'data mut T) -> NativeResult<R>,
    ) -> NativeResult<R> {
        cx.conversion
            .edit_managed_data(&self.root, self.native_type.contract(), edit)
    }

    pub fn get<V: FromKagari>(
        &self,
        cx: &mut NativeContext<'_>,
        field: &BoundField<T, V>,
    ) -> NativeResult<V> {
        field.check(&self.native_type)?;
        let mut conversion = ConversionContext::new(cx.runtime_mut(), field.native_type.owner())?;
        let value =
            conversion.managed_field::<T>(&self.root, self.native_type.contract(), field.slot)?;
        conversion.decode_prepared(field.type_argument(), &value)
    }

    pub fn set<V: IntoKagari>(
        &self,
        cx: &mut NativeContext<'_>,
        field: &BoundField<T, V>,
        value: V,
    ) -> NativeResult<()> {
        field.check(&self.native_type)?;
        cx.conversion.read_native_root::<Managed<T>, _>(
            &self.root,
            self.native_type.contract(),
            |_| Ok(()),
        )?;
        let mut conversion = ConversionContext::new(cx.runtime_mut(), field.native_type.owner())?;
        let value = conversion.encode_prepared(field.type_argument(), value)?;
        conversion.set_managed_field::<T>(
            &self.root,
            self.native_type.contract(),
            field.slot,
            &value,
        )
    }
}

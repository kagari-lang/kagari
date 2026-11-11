use crate::{
    native::{
        binding::NativeResult,
        conversion::{FromKagari, IntoKagari, context::ConversionContext},
        payload::{
            Opaque,
            data::NativeData,
            managed::{BoundField, Managed, invalid},
        },
        typed::NativeContext,
    },
    value::Value,
};

impl<T: NativeData> Opaque<Managed<T>> {
    /// Only fixed data is lent mutably; the traced fields remain inaccessible.
    pub fn edit_data<R>(
        &self,
        cx: &NativeContext<'_>,
        edit: impl for<'data> FnOnce(&'data mut T) -> NativeResult<R>,
    ) -> NativeResult<R> {
        cx.runtime().gc().ensure_no_native_borrow()?;
        cx.poll()?;
        let Value::GcHandle(id) = self.value(cx.runtime().gc())? else {
            return Err(invalid());
        };
        cx.runtime().gc().edit_managed_data(id, edit)
    }

    pub fn get<V: FromKagari>(
        &self,
        cx: &mut NativeContext<'_>,
        field: &BoundField<T, V>,
    ) -> NativeResult<V> {
        field.check(&self.native_type)?;
        cx.poll()?;
        let value = self.read(cx, |payload| {
            payload.values.get(field.slot).cloned().ok_or_else(invalid)
        })?;
        ConversionContext::new(cx.runtime_mut(), field.native_type.owner())?
            .decode_prepared(field.type_argument(), &value)
    }

    pub fn set<V: IntoKagari>(
        &self,
        cx: &mut NativeContext<'_>,
        field: &BoundField<T, V>,
        value: V,
    ) -> NativeResult<()> {
        field.check(&self.native_type)?;
        let Value::GcHandle(id) = self.value(cx.runtime().gc())? else {
            return Err(invalid());
        };
        let mut conversion = ConversionContext::new(cx.runtime_mut(), field.native_type.owner())?;
        conversion.argument_scope(|conversion| {
            let value = conversion.encode_prepared(field.type_argument(), value)?;
            conversion.runtime().gc().set_managed_field::<T>(
                id,
                self.native_type.schema()?,
                field.slot,
                value,
            )
        })
    }
}

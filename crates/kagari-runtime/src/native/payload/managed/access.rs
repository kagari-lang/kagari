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
    /// Read only the fixed data portion; traced fields remain behind checked getters.
    pub fn read_data<R>(
        &self,
        cx: &NativeContext<'_>,
        read: impl for<'a> FnOnce(&'a T) -> NativeResult<R>,
    ) -> NativeResult<R> {
        // SAFETY: Managed owns all edges separately; NativeData contains none.
        unsafe { self.read(cx, None, |payload, _| read(&payload.data)) }
    }

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
        field.check_schema(self.data.managed.as_ref().ok_or_else(invalid)?)?;
        cx.poll()?;
        // SAFETY: Retain the checked field before releasing the payload loan.
        let value = unsafe {
            self.read(cx, None, |payload, access| {
                let value = payload.values.get(field.slot).ok_or_else(invalid)?;
                access.retain(&payload.schema.fields[field.slot].contract, value)
            })?
        };
        value.decode(cx)
    }

    pub fn set<V: IntoKagari>(
        &self,
        cx: &mut NativeContext<'_>,
        field: &BoundField<T, V>,
        value: V,
    ) -> NativeResult<()> {
        field.check_schema(self.data.managed.as_ref().ok_or_else(invalid)?)?;
        let mut conversion = ConversionContext::new(cx.runtime(), field.native_type.owner())?;
        let Value::GcHandle(id) = self.value(cx.runtime().gc())? else {
            return Err(invalid());
        };
        conversion.argument_scope(|conversion| {
            let value = conversion.encode_prepared(field.type_argument(), value)?;
            cx.runtime().gc().set_managed_field::<T>(
                id,
                self.data.managed.as_ref().ok_or_else(invalid)?,
                field.slot,
                value,
            )
        })
    }
}

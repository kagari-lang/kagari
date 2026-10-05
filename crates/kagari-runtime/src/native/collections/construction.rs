//! Convenience factories; cache the runtime's prepared constructor for hot paths.
use crate::{
    frame::types::arguments::TypeArgument,
    native::{
        binding::NativeResult,
        collections::{map::ScriptMap, set::ScriptSet},
        conversion::KagariType,
        typed::NativeContext,
    },
};

impl NativeContext<'_> {
    pub fn create_map<K: KagariType + 'static, V: KagariType + 'static>(
        &mut self,
    ) -> NativeResult<ScriptMap<K, V>> {
        let key = self.conversion.type_for::<K>()?;
        let value = self.conversion.type_for::<V>()?;
        self.create_map_with_types(&key, &value)
    }

    /// Dynamic object mappings require their exact installed key/value scopes.
    pub fn create_map_with_types<K: KagariType + 'static, V: KagariType + 'static>(
        &mut self,
        key: &TypeArgument,
        value: &TypeArgument,
    ) -> NativeResult<ScriptMap<K, V>> {
        self.runtime()
            .bind_map_constructor(self.conversion.owner(), key, value)?
            .call(self, ())
    }

    pub fn create_set<T: KagariType + 'static>(&mut self) -> NativeResult<ScriptSet<T>> {
        let element = self.conversion.type_for::<T>()?;
        self.create_set_with_type(&element)
    }

    pub fn create_set_with_type<T: KagariType + 'static>(
        &mut self,
        element: &TypeArgument,
    ) -> NativeResult<ScriptSet<T>> {
        self.runtime()
            .bind_set_constructor(self.conversion.owner(), element)?
            .call(self, ())
    }
}

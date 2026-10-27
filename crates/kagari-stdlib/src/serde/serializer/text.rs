//! Formatting writes are charged before reserving a buffer, including collect_str.
use kagari_runtime::{
    error::RuntimeError,
    native::{binding::NativeResult, conversion::context::ConversionContext},
    value::Value,
};
use std::fmt::{self, Display, Write};

struct TextWriter<'scope, 'runtime> {
    cx: &'scope mut ConversionContext<'runtime>,
    text: String,
    error: Option<RuntimeError>,
}

impl Write for TextWriter<'_, '_> {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        if self.error.is_some() {
            return Err(fmt::Error);
        }
        if let Err(error) = self.cx.charge_string(text.len()) {
            self.error = Some(error);
            return Err(fmt::Error);
        }
        if self.text.try_reserve(text.len()).is_err() {
            self.error = Some(RuntimeError::resource_limit(
                "Serde formatted string capacity",
            ));
            return Err(fmt::Error);
        }
        self.text.push_str(text);
        Ok(())
    }
}

pub(super) fn collect<T: Display + ?Sized>(
    cx: &mut ConversionContext<'_>,
    value: &T,
) -> NativeResult<Value> {
    let mut writer = TextWriter {
        cx,
        text: String::new(),
        error: None,
    };
    let result = fmt::write(&mut writer, format_args!("{value}"));
    if let Some(error) = writer.error {
        return Err(error);
    }
    result.map_err(|_| RuntimeError::module_validation("Serde formatting failed"))?;
    writer.cx.runtime().gc().alloc_string(writer.text)
}

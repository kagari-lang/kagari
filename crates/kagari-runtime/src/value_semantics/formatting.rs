//! Bounded, non-executing previews read protected immutable fields in place.
use crate::{
    error::{RuntimeError, trap::TrapReason},
    value::{
        aggregates::{enumerations::EnumObject, tuples::TupleObject},
        carriers::ScopedValue,
        kind::ObjectKind,
        native::NativeObject,
        strings::StringObject,
    },
    value_semantics::storage_error,
};
use kagari_abi::representation::ValueType;
use kagari_gc::{access::Scope, error::GcError};
use kagari_types::ty::Ty;
use std::fmt::{self, Write};

struct Output<'a> {
    text: String,
    failed: bool,
    abort: Option<RuntimeError>,
    work: usize,
    poll: &'a mut dyn FnMut() -> Result<(), RuntimeError>,
}

impl Output<'_> {
    fn append(&mut self, text: &str) {
        if self.failed {
            return;
        }
        if text.len() > 1_048_576usize.saturating_sub(self.text.len()) {
            self.failed = true;
            return;
        }
        self.work = self.work.saturating_add(text.len());
        if self.work >= 4096 {
            self.work = 0;
            if let Err(error) = (self.poll)() {
                self.abort = Some(error);
                self.failed = true;
                return;
            }
        }
        if self.text.try_reserve(text.len()).is_err() {
            self.abort = Some(RuntimeError::resource_limit("value formatting allocation"));
            self.failed = true;
            return;
        }
        self.text.push_str(text);
    }
}

impl Write for Output<'_> {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        self.append(text);
        if self.failed { Err(fmt::Error) } else { Ok(()) }
    }
}

fn unsupported() -> RuntimeError {
    TrapReason::UnsupportedFormatValueInvalidHandleOrFormattingLimitExceeded.into()
}

pub(crate) fn format_value<'gc>(
    cx: &Scope<'gc>,
    poll: &mut dyn FnMut() -> Result<(), RuntimeError>,
    value: ScopedValue<'gc>,
    debug: bool,
) -> Result<String, RuntimeError> {
    let mut output = Output {
        text: String::new(),
        failed: false,
        abort: None,
        work: 0,
        poll,
    };
    let result = render(cx, value, debug, 0, &mut output);
    if let Some(error) = output.abort {
        return Err(error);
    }
    result?;
    if output.failed {
        return Err(unsupported());
    }
    Ok(output.text)
}

fn render<'gc>(
    cx: &Scope<'gc>,
    value: ScopedValue<'gc>,
    debug: bool,
    depth: usize,
    output: &mut Output<'_>,
) -> Result<(), RuntimeError> {
    if depth > 64 || output.failed {
        return Err(unsupported());
    }
    if let Some((kind, bits)) = value.scalar_payload() {
        let rendered = match kind {
            ValueType::Unit => output.write_str("()"),
            ValueType::Bool => write!(output, "{}", bits != 0),
            ValueType::Char => {
                let value = u32::try_from(bits)
                    .ok()
                    .and_then(char::from_u32)
                    .ok_or_else(unsupported)?;
                if debug {
                    write!(output, "{value:?}")
                } else {
                    output.write_char(value)
                }
            }
            ValueType::I32 | ValueType::I64 => write!(output, "{}", bits as i64),
            ValueType::U64 => write!(output, "{bits}"),
            ValueType::F32 => write!(output, "{}", f32::from_bits(bits as u32)),
            ValueType::F64 => write!(output, "{}", f64::from_bits(bits)),
            _ => return Err(unsupported()),
        };
        return rendered.map_err(|_| unsupported());
    }
    let object = value.object();
    let required = || object.ok_or_else(|| storage_error(GcError::Representation));
    match value.kind() {
        Some(ObjectKind::String) => {
            let text = cx
                .downcast::<StringObject>(required()?)
                .map_err(storage_error)?;
            cx.read(text, |text| {
                if debug {
                    write!(output, "{:?}", text.as_str())
                } else {
                    output.write_str(text.as_str())
                }
            })
            .map_err(|_| unsupported())?;
        }
        Some(ObjectKind::Tuple) if debug => {
            let tuple = cx
                .downcast::<TupleObject>(required()?)
                .map_err(storage_error)?;
            let count = cx.read(tuple, TupleObject::len);
            output.write_char('(').map_err(|_| unsupported())?;
            for index in 0..count {
                if index != 0 && index.is_multiple_of(1024) {
                    (output.poll)()?;
                }
                if index != 0 {
                    output.write_str(", ").map_err(|_| unsupported())?;
                }
                render(
                    cx,
                    TupleObject::get(cx, tuple, index).map_err(storage_error)?,
                    true,
                    depth + 1,
                    output,
                )?;
            }
            if count == 1 {
                output.write_char(',').map_err(|_| unsupported())?;
            }
            output.write_char(')').map_err(|_| unsupported())?;
        }
        Some(ObjectKind::Enum) if debug => {
            let enumeration = cx
                .downcast::<EnumObject>(required()?)
                .map_err(storage_error)?;
            cx.read(enumeration, |value| {
                let layout = value.layout();
                let owner = layout.module();
                let name = owner
                    .definition_name(layout.layout().declaration)
                    .ok_or_else(unsupported)?;
                let variant = owner
                    .definition_name(layout.variant().declaration)
                    .ok_or_else(unsupported)?;
                write!(output, "{name}::{variant}").map_err(|_| unsupported())
            })?;
            let count = cx.read(enumeration, EnumObject::len);
            if count != 0 {
                output.write_char('(').map_err(|_| unsupported())?;
                for index in 0..count {
                    if index != 0 && index.is_multiple_of(1024) {
                        (output.poll)()?;
                    }
                    if index != 0 {
                        output.write_str(", ").map_err(|_| unsupported())?;
                    }
                    render(
                        cx,
                        EnumObject::get(cx, enumeration, index).map_err(storage_error)?,
                        true,
                        depth + 1,
                        output,
                    )?;
                }
                output.write_char(')').map_err(|_| unsupported())?;
            }
        }
        Some(ObjectKind::Struct | ObjectKind::Array) if debug => {
            let name = if value.kind() == Some(ObjectKind::Struct) {
                "Struct"
            } else {
                "Array"
            };
            write!(output, "{name}@{:p}", required()?).map_err(|_| unsupported())?;
        }
        Some(ObjectKind::Native) if debug => {
            let object = cx
                .downcast::<NativeObject>(required()?)
                .map_err(storage_error)?;
            cx.read(object, |value| {
                let Ty::NativeObject(nominal) = value.applied().ty() else {
                    return Err(unsupported());
                };
                let name = value
                    .owner()
                    .definition_name(nominal.declaration)
                    .ok_or_else(unsupported)?;
                write!(output, "{name}@{object:p}").map_err(|_| unsupported())
            })?;
        }
        Some(ObjectKind::HostRoot) if debug => {
            output.write_str("<host>").map_err(|_| unsupported())?
        }
        Some(ObjectKind::HostPath) if debug => {
            output.write_str("<host path>").map_err(|_| unsupported())?
        }
        Some(ObjectKind::Interface) if debug => {
            output.write_str("<interface>").map_err(|_| unsupported())?
        }
        Some(ObjectKind::Closure) if debug => {
            output.write_str("<function>").map_err(|_| unsupported())?
        }
        Some(ObjectKind::Ephemeral) if debug => {
            output.write_str("<borrow>").map_err(|_| unsupported())?
        }
        _ => return Err(unsupported()),
    }
    Ok(())
}

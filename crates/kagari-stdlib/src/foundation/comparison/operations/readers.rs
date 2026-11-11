//! Checked collection readers retain values and dispatch targets across callbacks.
use kagari_runtime::{
    error::{RuntimeError, trap::TrapReason},
    native::{
        binding::NativeResult,
        interfaces::{Interface, binding::InterfaceMethod},
        typed::NativeContext,
        value_handle::ScriptValue,
    },
};

pub(super) struct Reader {
    value: Interface,
    len: InterfaceMethod<(), u64>,
    iter: InterfaceMethod<(), Interface>,
}

pub(super) struct Cursor {
    value: Interface,
    next: InterfaceMethod<(), Option<ScriptValue>>,
}

impl Reader {
    pub(super) fn interface(cx: &mut NativeContext<'_>, value: Interface) -> NativeResult<Self> {
        let runtime = cx.runtime_mut();
        let len = runtime.bind_interface_method(&value.member(runtime, "len")?)?;
        let iter = runtime.bind_interface_method(&value.member(runtime, "iter")?)?;
        Ok(Self { value, len, iter })
    }

    pub(super) fn len(&self, cx: &mut NativeContext<'_>) -> NativeResult<u64> {
        self.value.call(cx, &self.len, ())
    }

    pub(super) fn cursor(&self, cx: &mut NativeContext<'_>) -> NativeResult<Cursor> {
        let value = self.value.call(cx, &self.iter, ())?;
        let member = value.member(cx.runtime(), "next")?;
        let next = cx.runtime_mut().bind_interface_method(&member)?;
        Ok(Cursor { value, next })
    }
}

impl Cursor {
    pub(super) fn next(&self, cx: &mut NativeContext<'_>) -> NativeResult<Option<ScriptValue>> {
        cx.poll()?;
        self.value.call(cx, &self.next, ())
    }

    pub(super) fn item(&self, cx: &mut NativeContext<'_>) -> NativeResult<ScriptValue> {
        self.next(cx)?.ok_or_else(inconsistent_length)
    }

    pub(super) fn finish(&self, cx: &mut NativeContext<'_>) -> NativeResult<()> {
        if self.next(cx)?.is_some() {
            Err(inconsistent_length())
        } else {
            Ok(())
        }
    }
}

fn inconsistent_length() -> RuntimeError {
    RuntimeError::from(TrapReason::Provider(
        "collection iteration differs from its declared length".into(),
    ))
}

//! Checked collection readers retain values and dispatch targets across callbacks.
use kagari_runtime::{
    error::{RuntimeError, RuntimeErrorKind},
    native::{
        binding::NativeResult,
        declarations::SelectedCall,
        interfaces::{Interface, binding::InterfaceMethod},
        selected_method::SelectedMethod,
        typed::NativeContext,
        value_handle::ScriptValue,
    },
};

pub(super) enum Reader {
    Interface {
        value: Interface,
        len: InterfaceMethod<(), usize>,
        iter: InterfaceMethod<(), Interface>,
    },
    Concrete {
        value: ScriptValue,
        len: SelectedMethod<(ScriptValue,), usize>,
        iter: SelectedMethod<(ScriptValue,), ScriptValue>,
        next: SelectedMethod<(ScriptValue,), Option<ScriptValue>>,
    },
}

pub(super) enum Cursor {
    Interface {
        value: Interface,
        next: InterfaceMethod<(), Option<ScriptValue>>,
    },
    Concrete {
        value: ScriptValue,
        next: SelectedMethod<(ScriptValue,), Option<ScriptValue>>,
    },
}

impl Reader {
    pub(super) fn interface(cx: &NativeContext<'_>, value: Interface) -> NativeResult<Self> {
        let runtime = cx.runtime();
        let len = runtime.bind_interface_method(&value.member(runtime, "len")?)?;
        let iter = runtime.bind_interface_method(&value.member(runtime, "iter")?)?;
        Ok(Self::Interface { value, len, iter })
    }

    pub(super) fn concrete(
        cx: &NativeContext<'_>,
        value: ScriptValue,
        calls: &[SelectedCall],
    ) -> NativeResult<Self> {
        let [len, iter, next] = calls else {
            return Err(RuntimeError::module_validation(
                "collection reader requirements",
            ));
        };
        Ok(Self::Concrete {
            value,
            len: cx.selected_method(len)?,
            iter: cx.selected_method(iter)?,
            next: cx.selected_method(next)?,
        })
    }

    pub(super) fn len(&self, cx: &mut NativeContext<'_>) -> NativeResult<usize> {
        match self {
            Self::Interface { value, len, .. } => value.call(cx, len, ()),
            Self::Concrete { value, len, .. } => len.call(cx, (value.clone(),)),
        }
    }

    pub(super) fn cursor(&self, cx: &mut NativeContext<'_>) -> NativeResult<Cursor> {
        match self {
            Self::Interface { value, iter, .. } => {
                let value = value.call(cx, iter, ())?;
                let next = cx
                    .runtime()
                    .bind_interface_method(&value.member(cx.runtime(), "next")?)?;
                Ok(Cursor::Interface { value, next })
            }
            Self::Concrete {
                value, iter, next, ..
            } => Ok(Cursor::Concrete {
                value: iter.call(cx, (value.clone(),))?,
                next: next.clone(),
            }),
        }
    }
}

impl Cursor {
    pub(super) fn next(&self, cx: &mut NativeContext<'_>) -> NativeResult<Option<ScriptValue>> {
        cx.poll()?;
        match self {
            Self::Interface { value, next } => value.call(cx, next, ()),
            Self::Concrete { value, next } => next.call(cx, (value.clone(),)),
        }
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
    RuntimeError::new(
        RuntimeErrorKind::ScriptTrap,
        "collection iteration differs from its declared length",
    )
}

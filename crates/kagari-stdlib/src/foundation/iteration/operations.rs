//! Iterable terminal operations preserve ordered callbacks and completed effects.
use crate::foundation::enumerations::value::{self as enums, EnumSymbols, option};
use kagari_runtime::{
    error::RuntimeError,
    native::{
        binding::{Codec, NativeBinding, NativeResult},
        context::NativeCall,
        scalar::NativeScalar,
    },
    value::view::ValueView,
};
use kagari_types::{arithmetic::ArithmeticError, scalar::error::ScalarError};
use std::ops::ControlFlow;

#[derive(Clone, Copy)]
pub(crate) enum Terminal {
    Count,
    Find,
    Any,
    All,
    ForEach,
}

pub(crate) fn terminal_binding(terminal: Terminal, symbols: EnumSymbols) -> NativeBinding {
    let arity = if matches!(terminal, Terminal::Count) {
        1
    } else {
        2
    };
    NativeBinding::new(vec![Codec::Value; arity], Codec::Value, move |cx| {
        consume(&symbols, cx, terminal)
    })
}

pub(crate) fn identity<'call>(cx: &mut NativeCall<'call>) -> NativeResult<ValueView<'call>> {
    cx.argument(0)
}

pub(crate) fn destination<'call>(cx: &mut NativeCall<'call>) -> NativeResult<ValueView<'call>> {
    let source = cx.argument(0)?;
    cx.call_values(cx.selected_at(0)?, &[source])
}

pub(crate) fn fold<'call>(
    symbols: &EnumSymbols,
    cx: &mut NativeCall<'call>,
) -> NativeResult<ValueView<'call>> {
    let callback = cx.callable(2)?;
    let accumulator = cx.argument(1)?;
    let source = cx.argument(0)?;
    let cursor = cx.call_values(cx.selected_at(0)?, &[source])?;
    cx.with_value_loop(&accumulator, |cx, accumulator| {
        let result = cx.call_values(cx.selected_at(1)?, &[cursor])?;
        let Some(item) = enums::read_option(symbols, cx, &result)? else {
            return Ok(ControlFlow::Break(accumulator));
        };
        callback
            .call_values(cx, &[accumulator, item])
            .map(ControlFlow::Continue)
    })
}

fn consume<'call>(
    symbols: &EnumSymbols,
    cx: &mut NativeCall<'call>,
    terminal: Terminal,
) -> NativeResult<ValueView<'call>> {
    let callback = if matches!(terminal, Terminal::Count) {
        None
    } else {
        Some(cx.callable(1)?)
    };
    let source = cx.argument(0)?;
    let cursor = cx.call_values(cx.selected_at(0)?, &[source])?;
    let mut count = 0u64;
    loop {
        let completed = cx.with_optional_value_temporaries(|cx| {
            let result = cx.call_values(cx.selected_at(1)?, &[cursor])?;
            let Some(item) = enums::read_option(symbols, cx, &result)? else {
                return match terminal {
                    Terminal::Count => Ok(Some(count.encode())),
                    Terminal::Find => option(symbols, cx, None).map(Some),
                    Terminal::Any => Ok(Some(false.encode())),
                    Terminal::All => Ok(Some(true.encode())),
                    Terminal::ForEach => Ok(Some(ValueView::unit())),
                };
            };
            if matches!(terminal, Terminal::Count) {
                count = increment(count)?;
                return Ok(None);
            }
            let result = callback
                .as_ref()
                .ok_or_else(invalid)?
                .call_values(cx, &[item])?;
            match terminal {
                Terminal::Find if bool::decode(result)? => {
                    option(symbols, cx, Some(item)).map(Some)
                }
                Terminal::Any if bool::decode(result)? => Ok(Some(true.encode())),
                Terminal::All if !bool::decode(result)? => Ok(Some(false.encode())),
                _ => Ok(None),
            }
        })?;
        if let Some(value) = completed {
            return Ok(value);
        }
    }
}

fn invalid() -> RuntimeError {
    RuntimeError::module_validation("Iterable adapter contract")
}

pub(super) fn increment(value: u64) -> NativeResult<u64> {
    value
        .checked_add(1)
        .ok_or_else(|| RuntimeError::from(ScalarError::from(ArithmeticError::Overflow)))
}

#[cfg(test)]
mod tests {
    use super::increment;
    use kagari_runtime::error::RuntimeErrorKind;

    #[test]
    fn logical_counts_use_the_full_u64_domain() {
        assert_eq!(increment(u64::from(u32::MAX)).unwrap(), 1u64 << 32);
        assert_eq!(increment(u64::MAX - 1).unwrap(), u64::MAX);
        assert_eq!(
            increment(u64::MAX).unwrap_err().kind(),
            RuntimeErrorKind::ScriptTrap
        );
    }
}

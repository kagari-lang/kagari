//! Iterable terminal operations preserve ordered callbacks and completed effects.
use crate::foundation::enumerations::value::{self as enums, EnumSymbols, option};
use kagari_contract::operations::IterOp;
use kagari_runtime::{
    error::{RuntimeError, RuntimeErrorKind},
    native::{
        binding::{Codec, NativeBinding, NativeResult},
        context::CallContext,
        scalar::NativeScalar,
    },
    value::Value,
};

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

pub(crate) fn identity(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    cx.argument(0)
}

pub(crate) fn destination(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    cx.call_values(cx.selected_at(0)?, &[cx.argument(0)?])
}

pub(crate) fn fold(symbols: &EnumSymbols, cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let callback = cx.callable(2)?.store();
    let mut accumulator = cx.argument(1)?;
    let root = cx.heap().root_value(accumulator).ok_or_else(invalid)?;
    let cursor = cx.call_values(cx.selected_at(0)?, &[cx.argument(0)?])?;
    let _cursor = cx.heap().root_value(cursor).ok_or_else(invalid)?;
    loop {
        cx.poll()?;
        let result = cx.call_values(cx.selected_at(1)?, &[cursor])?;
        let Some(item) = enums::read_option(symbols, cx, &result)? else {
            return Ok(accumulator);
        };
        let _item = cx.heap().root_value(item).ok_or_else(invalid)?;
        accumulator = callback.call_values(cx, &[accumulator, item])?;
        root.set(cx.heap(), accumulator).ok_or_else(invalid)?;
    }
}

fn consume(
    symbols: &EnumSymbols,
    cx: &mut CallContext<'_>,
    terminal: Terminal,
) -> NativeResult<Value> {
    let callback = if matches!(terminal, Terminal::Count) {
        None
    } else {
        Some(cx.callable(1)?.store())
    };
    let cursor = cx.call_values(cx.selected_at(0)?, &[cx.argument(0)?])?;
    let _cursor = cx.heap().root_value(cursor).ok_or_else(invalid)?;
    let mut count = 0u64;
    loop {
        cx.poll()?;
        let result = cx.call_values(cx.selected_at(1)?, &[cursor])?;
        let Some(item) = enums::read_option(symbols, cx, &result)? else {
            return match terminal {
                Terminal::Count => Ok(count.encode()),
                Terminal::Find => option(symbols, cx, None),
                Terminal::Any => Ok(false.encode()),
                Terminal::All => Ok(true.encode()),
                Terminal::ForEach => Ok(Value::Unit),
            };
        };
        let _item = cx.heap().root_value(item).ok_or_else(invalid)?;
        if matches!(terminal, Terminal::Count) {
            count = increment(count)?;
            continue;
        }
        let result = callback
            .as_ref()
            .ok_or_else(invalid)?
            .call_values(cx, &[item])?;
        match terminal {
            Terminal::Find if bool::decode(result)? => return option(symbols, cx, Some(item)),
            Terminal::Any if bool::decode(result)? => return Ok(true.encode()),
            Terminal::All if !bool::decode(result)? => return Ok(false.encode()),
            _ => {}
        }
    }
}

fn invalid() -> RuntimeError {
    RuntimeError::module_validation("Iterable adapter contract")
}

pub(super) fn increment(value: u64) -> NativeResult<u64> {
    value
        .checked_add(1)
        .ok_or_else(|| RuntimeError::new(RuntimeErrorKind::ScriptTrap, "integer overflow"))
}

pub(crate) fn iter(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    cx.iter_operation(0, IterOp::New)
}

pub(crate) fn next(symbols: &EnumSymbols, cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let (some, none) = symbols.option_members()?;
    cx.iterator_next_result(0, some, none)
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

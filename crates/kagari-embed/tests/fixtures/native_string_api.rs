//! An application-owned text algorithm uses the same bounded buffer.
use kagari_native_macros::native_module;

#[native_module("game::text")]
pub mod text {
    use kagari_common::cancellation::CancellationToken;
    use kagari_runtime::native_value::{
        NativeCall, NativeResult,
        text::{NativeTextBuffer, size_error},
    };
    use std::cell::RefCell;
    #[cfg(test)]
    use std::mem;
    thread_local! { static EVENTS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) }; }
    thread_local! { static CANCEL: RefCell<Option<CancellationToken>> = const { RefCell::new(None) }; }

    /// An application-owned name for the checked UTF-8 engine representation.
    #[native_type]
    pub type Text = String;

    /// Reverse Unicode scalars through the shared text buffer, preserving UTF-8.
    #[native]
    pub fn reverse(text: Text, #[context] call: &NativeCall) -> NativeResult<Text> {
        call.charge_work(text.len() as u64)?;
        let mut buffer = NativeTextBuffer::new(call, text.len())?;
        for character in text.chars().rev() {
            buffer.push_char(character)?;
        }
        Ok(buffer.finish())
    }

    /// Compose a prefix and text using checked output capacity.
    #[native]
    pub fn prefix(
        text: String,
        prefix: String,
        #[context] call: &NativeCall,
    ) -> NativeResult<String> {
        let mut buffer = NativeTextBuffer::new(
            call,
            text.len()
                .checked_add(prefix.len())
                .ok_or_else(size_error)?,
        )?;
        buffer.push_str(&prefix)?;
        buffer.push_str(&text)?;
        Ok(buffer.finish())
    }

    /// Deliberately underreserve to prove that a buggy provider cannot grow unchecked.
    #[native]
    pub fn overfill(text: String, #[context] call: &NativeCall) -> NativeResult<String> {
        let mut buffer = NativeTextBuffer::new(call, 0)?;
        buffer.push_str(&text)?;
        Ok(buffer.finish())
    }

    /// Exercise cancellation after output reservation but before the next append.
    #[native]
    pub fn cancel_after_reserve(#[context] call: &NativeCall) -> NativeResult<String> {
        let mut buffer = NativeTextBuffer::new(call, 1)?;
        CANCEL.with(|token| {
            if let Some(token) = token.borrow_mut().take() {
                token.cancel();
            }
        });
        buffer.push_str("x")?;
        EVENTS.with(|events| events.borrow_mut().push("appended".into()));
        Ok(buffer.finish())
    }

    /// Observe an eager argument in host-owned state and return its immutable text.
    #[native]
    pub fn record(text: String) -> String {
        EVENTS.with(|events| events.borrow_mut().push(text.clone()));
        text
    }

    #[cfg(test)]
    pub fn take_events() -> Vec<String> {
        EVENTS.with(|events| mem::take(&mut *events.borrow_mut()))
    }

    #[cfg(test)]
    pub fn cancel_with(token: CancellationToken) {
        CANCEL.with(|slot| *slot.borrow_mut() = Some(token));
    }
}

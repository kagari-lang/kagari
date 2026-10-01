//! Bounded, fallible text construction shared by native providers.
use crate::{
    error::RuntimeError,
    native_value::{NativeCall, NativeResult},
};

/// Reserve and prepay output-byte work before allocation. Appends cannot exceed
/// the declared capacity or trigger another allocation. This buffer owns Rust
/// text, never a mutable reference into the script heap.
pub struct NativeTextBuffer {
    call: NativeCall,
    text: String,
    limit: usize,
}

impl NativeTextBuffer {
    pub fn new(call: &NativeCall, bytes: usize) -> NativeResult<Self> {
        call.charge_work(u64::try_from(bytes).map_err(|_| size_error())?)?;
        let mut text = String::new();
        text.try_reserve_exact(bytes).map_err(|_| size_error())?;
        Ok(Self {
            call: call.clone(),
            text,
            limit: bytes,
        })
    }

    pub fn push_str(&mut self, text: &str) -> NativeResult<()> {
        self.check_append(text.len())?;
        self.text.push_str(text);
        Ok(())
    }

    pub fn push_char(&mut self, character: char) -> NativeResult<()> {
        self.check_append(character.len_utf8())?;
        self.text.push(character);
        Ok(())
    }

    pub fn finish(self) -> String {
        self.text
    }

    fn check_append(&self, bytes: usize) -> NativeResult<()> {
        // Output work is prepaid, but long provider loops must still observe
        // cancellation and execution deadlines between appends.
        self.call.charge_work(0)?;
        if self
            .text
            .len()
            .checked_add(bytes)
            .is_some_and(|length| length <= self.limit)
        {
            Ok(())
        } else {
            Err(RuntimeError::module_validation(
                "native text exceeded its reserved capacity",
            ))
        }
    }
}

pub fn copy_text(call: &NativeCall, text: &str) -> NativeResult<String> {
    let mut buffer = NativeTextBuffer::new(call, text.len())?;
    buffer.push_str(text)?;
    Ok(buffer.finish())
}

pub fn size_error() -> RuntimeError {
    RuntimeError::resource_limit("native text result size")
}

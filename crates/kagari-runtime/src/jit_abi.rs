use crate::{Runtime, value::Value};

pub const JIT_CONSUME_INSTRUCTION_STEP_SYMBOL: &str = "kagari_runtime.consume_instruction_step";

pub const JIT_STATUS_OK: i32 = 0;
pub const JIT_STATUS_RUNTIME_ERROR: i32 = 1;
pub const JIT_VALUE_TAG_UNIT: u8 = 0;
pub const JIT_VALUE_TAG_BOOL: u8 = 1;
pub const JIT_VALUE_TAG_I32: u8 = 2;

pub type JitCompiledFunction =
    unsafe extern "C" fn(runtime: *const Runtime, result: *mut JitValue) -> i32;

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JitValue {
    pub tag: u8,
    pub _padding: [u8; 7],
    pub payload: i64,
}

impl JitValue {
    pub fn unit() -> Self {
        Self {
            tag: JIT_VALUE_TAG_UNIT,
            _padding: [0; 7],
            payload: 0,
        }
    }

    pub fn bool(value: bool) -> Self {
        Self {
            tag: JIT_VALUE_TAG_BOOL,
            _padding: [0; 7],
            payload: if value { 1 } else { 0 },
        }
    }

    pub fn i32(value: i32) -> Self {
        Self {
            tag: JIT_VALUE_TAG_I32,
            _padding: [0; 7],
            payload: i64::from(value),
        }
    }

    pub fn into_value(self) -> Option<Value> {
        match self.tag {
            JIT_VALUE_TAG_UNIT => Some(Value::Unit),
            JIT_VALUE_TAG_BOOL => Some(Value::Bool(self.payload != 0)),
            JIT_VALUE_TAG_I32 => i32::try_from(self.payload).ok().map(Value::I32),
            _ => None,
        }
    }
}

impl Default for JitValue {
    fn default() -> Self {
        Self::unit()
    }
}

pub extern "C" fn jit_consume_instruction_step(runtime: *const Runtime) -> i32 {
    let Some(runtime) = (unsafe { runtime.as_ref() }) else {
        return JIT_STATUS_RUNTIME_ERROR;
    };
    match runtime.consume_instruction_step() {
        Ok(()) => JIT_STATUS_OK,
        Err(_) => JIT_STATUS_RUNTIME_ERROR,
    }
}

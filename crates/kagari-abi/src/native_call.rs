use std::ffi::c_void;
pub const JIT_CONSUME_INSTRUCTION_STEP_SYMBOL: &str = "kagari_runtime.consume_instruction_step";

pub const JIT_STATUS_OK: i32 = 0;

pub const JIT_STATUS_RESOURCE_LIMIT: i32 = 1;

pub const JIT_STATUS_INTEGER_OVERFLOW: i32 = 2;

pub const JIT_STATUS_INVALID_RUNTIME: i32 = 3;

pub const JIT_STATUS_INVALID_HEAP_REFERENCE: i32 = 4;

pub const JIT_STATUS_ENGINE_FAULT: i32 = 5;

pub const JIT_STATUS_CANCELLED: i32 = 6;

pub const JIT_VALUE_TAG_UNIT: u8 = 0;

pub const JIT_VALUE_TAG_BOOL: u8 = 1;

pub const JIT_VALUE_TAG_I32: u8 = 2;

pub type JitCompiledFunction =
    unsafe extern "C" fn(runtime: *const c_void, result: *mut JitValue) -> i32;

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
}

impl Default for JitValue {
    fn default() -> Self {
        Self::unit()
    }
}

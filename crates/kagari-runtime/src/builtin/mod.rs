pub mod standard;

use kagari_contract::standard::RuntimePrimitive;
use std::borrow::Cow;

use crate::{
    Runtime,
    error::{RuntimeError, RuntimeErrorKind},
    module::LoadedModule,
    value::Value,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltinError {
    error: RuntimeError,
}

impl BuiltinError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            error: RuntimeError::new(RuntimeErrorKind::ScriptTrap, message),
        }
    }

    pub fn message(&self) -> &str {
        self.error.message()
    }

    pub fn into_runtime_error(self) -> RuntimeError {
        self.error
    }

    pub fn kind(&self) -> RuntimeErrorKind {
        self.error.kind()
    }

    fn with_context(self, name: &str) -> Self {
        RuntimeError::new(self.error.kind(), format!("{name}: {}", self.message())).into()
    }
}

impl From<RuntimeError> for BuiltinError {
    fn from(error: RuntimeError) -> Self {
        Self { error }
    }
}

pub fn invoke_standard(
    runtime: &Runtime,
    owner: &LoadedModule,
    intrinsic: RuntimePrimitive,
    args: &[Value],
) -> Result<Value, BuiltinError> {
    standard::invoke(runtime, owner, intrinsic, args)
        .map_err(|err| err.with_context(&standard_intrinsic_name(intrinsic)))
}

fn standard_intrinsic_name(intrinsic: RuntimePrimitive) -> Cow<'static, str> {
    Cow::Owned(format!("{intrinsic:?}"))
}

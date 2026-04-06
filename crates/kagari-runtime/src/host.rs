use std::{fmt, sync::Arc};

use crate::value::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HostObjectId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostPassingStyle {
    Owned,
    SharedBorrow,
    UniqueBorrow,
}

#[derive(Debug, Clone)]
pub struct HostParameter {
    pub name: &'static str,
    pub type_name: &'static str,
    pub passing: HostPassingStyle,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostError {
    message: String,
}

impl HostError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

pub type HostCallback = dyn Fn(&[Value]) -> Result<Value, HostError> + Send + Sync + 'static;

#[derive(Clone)]
pub struct HostFunction {
    pub symbol: &'static str,
    pub params: Vec<HostParameter>,
    pub return_type: &'static str,
    handler: Arc<HostCallback>,
}

impl fmt::Debug for HostFunction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HostFunction")
            .field("symbol", &self.symbol)
            .field("params", &self.params)
            .field("return_type", &self.return_type)
            .finish_non_exhaustive()
    }
}

impl HostFunction {
    pub fn new(
        symbol: &'static str,
        params: Vec<HostParameter>,
        return_type: &'static str,
        handler: impl Fn(&[Value]) -> Result<Value, HostError> + Send + Sync + 'static,
    ) -> Self {
        Self {
            symbol,
            params,
            return_type,
            handler: Arc::new(handler),
        }
    }

    pub fn invoke(&self, args: &[Value]) -> Result<Value, HostError> {
        (self.handler)(args)
    }
}

#[derive(Debug, Default)]
pub struct HostRegistry {
    functions: Vec<HostFunction>,
}

impl HostRegistry {
    pub fn register(&mut self, function: HostFunction) {
        self.functions.push(function);
    }

    pub fn functions(&self) -> &[HostFunction] {
        &self.functions
    }

    pub fn invoke(&self, symbol: &str, args: &[Value]) -> Result<Value, HostError> {
        let function = self
            .functions
            .iter()
            .find(|function| function.symbol == symbol)
            .ok_or_else(|| HostError::new(format!("unknown host function `{symbol}`")))?;
        function.invoke(args)
    }
}

#[derive(Debug)]
pub struct SharedHostRef<'host, T: ?Sized> {
    value: &'host T,
}

impl<'host, T: ?Sized> SharedHostRef<'host, T> {
    pub fn new(value: &'host T) -> Self {
        Self { value }
    }

    pub fn get(&self) -> &'host T {
        self.value
    }
}

#[derive(Debug)]
pub struct MutHostRef<'host, T: ?Sized> {
    value: &'host mut T,
}

impl<'host, T: ?Sized> MutHostRef<'host, T> {
    pub fn new(value: &'host mut T) -> Self {
        Self { value }
    }

    pub fn get(&self) -> &T {
        self.value
    }

    pub fn get_mut(&mut self) -> &mut T {
        self.value
    }
}

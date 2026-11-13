//! Ready-only access to installation-owned persistent cells.
use crate::{module::ExecutableModule, value::carriers::RootedValue};
pub(crate) mod access;
pub(super) mod cells;
mod compatibility;
pub(crate) mod initialization;

use kagari_common::span::Span;
use kagari_contract::ids::FunctionRef;
use std::{collections::TryReserveError, fmt};

/// A fixed introducing-generation recipe. A copy grants no initialization authority.
#[derive(Debug, Clone)]
pub struct StaticRecipe {
    pub module: ExecutableModule,
    pub function: FunctionRef,
}

/// Detached diagnostic state. Only runtime-owned transitions can change a cell.
#[derive(Debug, Clone)]
pub enum StaticState {
    Uninitialized(StaticRecipe),
    Initializing(StaticRecipe),
    Ready(RootedValue),
    /// Bounded, owned diagnostic text; no frame, execution session or borrow is retained.
    Failed(StaticFailure),
}

/// Bounded diagnostic data, independent of frames, sessions and executable ownership.
#[derive(Debug, Clone)]
pub struct StaticFailure {
    cause: String,
    origin: Option<Span>,
}

impl StaticFailure {
    /// Retain at most 1024 UTF-8 bytes of the cause and an optional initializer span.
    /// Constructing a diagnostic does not grant access to a cell or its transitions.
    pub fn new(cause: &str, origin: Option<Span>) -> Self {
        let mut end = cause.len().min(1024);
        while !cause.is_char_boundary(end) {
            end -= 1;
        }
        Self {
            cause: cause[..end].to_owned(),
            origin,
        }
    }

    pub fn cause(&self) -> &str {
        &self.cause
    }

    pub(in crate::module) fn retained_bytes(&self) -> usize {
        self.cause.capacity()
    }

    pub(in crate::module) fn try_copy(&self) -> Result<Self, TryReserveError> {
        let mut cause = String::new();
        cause.try_reserve_exact(self.cause.len())?;
        cause.push_str(&self.cause);
        Ok(Self {
            cause,
            origin: self.origin,
        })
    }

    pub fn origin(&self) -> Option<Span> {
        self.origin
    }
}

impl fmt::Display for StaticFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.cause)
    }
}

#[cfg(test)]
pub(crate) mod tests;

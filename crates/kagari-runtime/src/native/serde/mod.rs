//! Error integration for external Serde adapters.
use crate::error::RuntimeError;
use serde::{de, ser};
use std::fmt;

impl ser::Error for RuntimeError {
    fn custom<T: fmt::Display>(message: T) -> Self {
        Self::module_validation(format!("Serde conversion: {message}"))
    }
}

impl de::Error for RuntimeError {
    fn custom<T: fmt::Display>(message: T) -> Self {
        Self::module_validation(format!("Serde conversion: {message}"))
    }
}

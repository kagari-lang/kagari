//! Explicit declarations, checked Rust bindings and cold native producers.
pub mod adapter;
pub(crate) mod application;
pub mod arguments;
pub mod assembly;
pub mod binding;
pub mod builder;
pub mod callable;
pub mod catalog;
pub mod completion;
pub mod composition;
pub mod context;
pub mod conversion;
pub mod declarations;
pub mod enums;
pub mod function_handle;
pub mod functions;
pub mod future;
mod host;
pub mod installation;
pub mod interfaces;
pub mod methods;
pub mod module;
pub mod objects;
pub mod payload;
pub mod primitive;
pub mod projection;
pub(crate) mod registry;
mod result;
pub mod returns;
pub mod rust_type;
pub mod scalar;
pub mod schema;
mod selected;
pub mod selected_method;
#[cfg(feature = "serde")]
mod serde;
pub mod storage;
pub mod storage_type;
pub(crate) mod stored_selection;
pub mod typed;
pub mod types;
pub mod value_handle;
pub mod views;

#[cfg(test)]
mod tests;

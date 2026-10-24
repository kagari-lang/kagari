//! Bounded snapshots of an explicit public DTO root, not a Runtime or heap image.
//!
//! The codec invokes no script or host callbacks. It rejects compound aliases,
//! cycles, private fields and unsupported capabilities instead of duplicating
//! object identity or serializing handles. Statics are not enumerated or initialized.
//! Hosts must first stop all writers; compatible-publication admission alone does
//! not establish state quiescence. Application projections and resource validation
//! remain explicit host responsibilities.

mod codec;
pub mod data;
pub mod error;
pub mod limits;
mod migration;
mod wire;

use crate::{
    frame::types::arguments::TypeArgument,
    native::{conversion::context::ConversionContext, value_handle::ScriptValue},
    state::{
        data::{SchemaVersion, SnapshotHeader, StateSchema, StateSnapshot},
        error::SnapshotResult,
        limits::SnapshotOptions,
    },
};

/// Describe the complete checked DTO contract, including empty or unselected branches.
/// Unsupported types and private fields reject without executing code or allocating
/// script objects. The resulting schema contains no runtime identity or lease.
pub fn describe(
    cx: &mut ConversionContext<'_>,
    expected: &TypeArgument,
    version: SchemaVersion,
    options: &SnapshotOptions,
) -> SnapshotResult<StateSchema> {
    codec::describe(cx, expected, version, options)
}

/// Export a rooted DTO as owned data under explicit size, work and cancellation limits.
/// The root must belong to the supplied runtime and checked type scope. Repeated
/// compound identities report distinct alias or cycle errors with both paths;
/// repeated scalar strings are permitted. External aliases require host quiescence.
pub fn export(
    cx: &mut ConversionContext<'_>,
    root: &ScriptValue,
    header: SnapshotHeader,
    options: &SnapshotOptions,
) -> SnapshotResult<StateSnapshot> {
    codec::export(cx, root, header, options)
}

/// Validate a snapshot against the complete destination contract and restore fresh objects.
/// Schema IDs, versions and shapes must match exactly; migration is a separate explicit
/// operation. No existing object is mutated. Failure releases temporary roots, and the
/// returned value is rooted before construction protection ends. Business-ID validation
/// and any resource reattachment are separate application operations.
pub fn restore(
    cx: &mut ConversionContext<'_>,
    expected: &TypeArgument,
    snapshot: &StateSnapshot,
    required: &SchemaVersion,
    options: &SnapshotOptions,
) -> SnapshotResult<ScriptValue> {
    codec::restore(cx, expected, snapshot, required, options)
}

//! Callback-free bridge between checked runtime values and portable state trees.
mod export;
#[cfg(test)]
mod polling_tests;
mod restore;
mod scalar;
mod schema;
mod walk;

use crate::{
    frame::types::arguments::TypeArgument,
    native::{conversion::context::ConversionContext, value_handle::ScriptValue},
    state::{
        codec::walk::Walk,
        data::{SchemaVersion, SnapshotHeader, StateSchema, StateSnapshot},
        error::{SnapshotErrorKind, SnapshotResult, SnapshotStage},
        limits::{SnapshotOptions, schemas_equal, validate_schema, validate_snapshot},
    },
};

pub(super) fn describe(
    cx: &mut ConversionContext<'_>,
    expected: &TypeArgument,
    version: SchemaVersion,
    options: &SnapshotOptions,
) -> SnapshotResult<StateSchema> {
    let mut walk = Walk::new(cx, options, SnapshotStage::Describe);
    let result = (|| {
        let schema = schema::describe(cx, expected, version, &mut walk)?;
        walk.budget.reset_data_counts();
        validate_schema(&schema, &mut walk.budget)?;
        walk.poll(cx)?;
        Ok(schema)
    })();
    walk.finish(cx, result)
}

pub(super) fn export(
    cx: &mut ConversionContext<'_>,
    root: &ScriptValue,
    header: SnapshotHeader,
    options: &SnapshotOptions,
) -> SnapshotResult<StateSnapshot> {
    let mut walk = Walk::new(cx, options, SnapshotStage::Export);
    let result = (|| {
        walk.poll(cx)?;
        walk.budget.work(1)?;
        let value = walk.runtime(root.snapshot_value(cx))?;
        let version = SchemaVersion {
            id: walk.copy_text(cx, &header.schema.id, true)?,
            version: header.schema.version,
        };
        let schema = schema::describe(cx, root.type_argument(), version, &mut walk)?;
        walk.budget.reset_data_counts();
        validate_schema(&schema, &mut walk.budget)?;
        walk.budget.reset_data_counts();
        let payload = export::export(cx, root.type_argument(), &value, &schema, &mut walk)?;
        walk.budget.reset_data_counts();
        validate_snapshot(&header, &schema, &payload, &mut walk.budget)?;
        walk.poll(cx)?;
        Ok(StateSnapshot::from_validated(header, schema, payload))
    })();
    walk.finish(cx, result)
}

pub(super) fn restore(
    cx: &mut ConversionContext<'_>,
    expected: &TypeArgument,
    snapshot: &StateSnapshot,
    required: &SchemaVersion,
    options: &SnapshotOptions,
) -> SnapshotResult<ScriptValue> {
    let mut walk = Walk::new(cx, options, SnapshotStage::Restore);
    let result = (|| {
        walk.poll(cx)?;
        validate_snapshot(
            snapshot.header(),
            snapshot.schema(),
            snapshot.payload(),
            &mut walk.budget,
        )?;
        let version = SchemaVersion {
            id: walk.copy_text(cx, &required.id, true)?,
            version: required.version,
        };
        if snapshot.header().schema.version != required.version
            || !walk
                .compare_text(cx, &snapshot.header().schema.id, &required.id)?
                .is_eq()
        {
            return Err(walk.error(
                SnapshotErrorKind::SchemaMismatch,
                "required schema ID/version differs",
            ));
        }
        walk.budget.reset_data_counts();
        let schema = schema::describe(cx, expected, version, &mut walk)?;
        walk.budget.reset_data_counts();
        validate_schema(&schema, &mut walk.budget)?;
        // Both complete schemas have been admitted before script-heap allocation.
        if !schemas_equal(&schema, snapshot.schema(), &mut walk.budget)? {
            return Err(walk.error(
                SnapshotErrorKind::SchemaMismatch,
                "complete checked schema differs from snapshot",
            ));
        }
        walk.budget.reset_data_counts();
        restore::restore(cx, expected, snapshot.payload(), &mut walk)
    })();
    walk.finish(cx, result)
}

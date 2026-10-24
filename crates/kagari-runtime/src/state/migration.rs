use crate::state::{
    data::{
        MigrationContext, SchemaVersion, SnapshotHeader, StateSchema, StateSnapshot, StateValue,
    },
    error::{SnapshotErrorKind, SnapshotResult, SnapshotStage},
    limits::{
        Budget, SnapshotOptions, schemas_equal, validate_schema, validate_snapshot, versions_equal,
    },
};
use kagari_common::cancellation::CancellationToken;

mod schema_copy;

impl MigrationContext<'_> {
    /// Original provenance, including the exact source logical schema version.
    pub fn source(&self) -> &SnapshotHeader {
        self.source
    }

    pub fn target(&self) -> &StateSchema {
        self.target
    }

    pub fn cancellation(&self) -> &CancellationToken {
        self.budget.cancellation()
    }

    pub fn check_cancelled(&self) -> SnapshotResult<()> {
        self.budget.cancel()
    }

    /// Charge cooperative host work. Arbitrary callback execution is not metered.
    pub fn charge_work(&mut self, units: u64) -> SnapshotResult<()> {
        self.budget.work(units)
    }

    /// Admit and copy a data string with chunked cancellation checks.
    pub fn copy_string(&mut self, value: &str) -> SnapshotResult<String> {
        self.budget.copy_string(value, false)
    }

    /// Reserve a bounded DTO sequence; final output is independently validated.
    pub fn sequence(&mut self, capacity: usize) -> SnapshotResult<Vec<StateValue>> {
        self.budget.admit_children(capacity, false)?;
        let mut values = Vec::new();
        self.budget.reserve(&mut values, capacity)?;
        Ok(values)
    }
}

impl StateSnapshot {
    /// Consume the old DTO and explicitly migrate to one complete target schema.
    ///
    /// Source and target validation precede the trusted, cooperative callback.
    /// Failure or unwind consumes this owned snapshot; no runtime state is mutated.
    /// The hook is not sandboxed, and output validation cannot bound its execution.
    pub fn migrate(
        self,
        expected_from: &SchemaVersion,
        target: &StateSchema,
        options: &SnapshotOptions,
        hook: impl FnOnce(&mut MigrationContext<'_>, StateValue) -> SnapshotResult<StateValue>,
    ) -> SnapshotResult<Self> {
        let mut budget = Budget::new(options, SnapshotStage::Migrate);
        let (mut header, source_schema, payload) = self.into_parts();
        validate_snapshot(&header, &source_schema, &payload, &mut budget)?;
        budget.count(
            expected_from.id.len(),
            budget.limits.max_identifier_bytes,
            "identifier_bytes",
        )?;
        if !versions_equal(expected_from, &header.schema, &mut budget)? {
            return Err(budget.error(
                SnapshotErrorKind::SchemaMismatch,
                "migration source version differs",
            ));
        }
        budget.reset_data_counts();
        validate_schema(target, &mut budget)?;
        if versions_equal(&target.version, &header.schema, &mut budget)?
            && !schemas_equal(&source_schema, target, &mut budget)?
        {
            return Err(budget.error(
                SnapshotErrorKind::SchemaMismatch,
                "changed schema requires a changed logical ID/version",
            ));
        }
        budget.reset_data_counts();
        let mut context = MigrationContext {
            source: &header,
            target,
            budget,
        };
        context.check_cancelled()?;
        let output = hook(&mut context, payload)?;
        context.check_cancelled()?;
        let mut budget = context.budget;
        budget.reset_data_counts();
        let schema = schema_copy::copy(target, &mut budget)?;
        header.schema = SchemaVersion {
            id: budget.copy_string(&schema.version.id, true)?,
            version: schema.version.version,
        };
        budget.reset_data_counts();
        Self::new(header, schema, output, &mut budget)
    }
}

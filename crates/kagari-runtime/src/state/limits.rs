//! Finite operation admission, work accounting and cooperative cancellation.

use crate::state::{
    data::{SchemaVersion, SnapshotHeader, StateSchema, StateValue},
    error::{SnapshotError, SnapshotErrorKind, SnapshotResult, SnapshotStage},
};
use kagari_common::cancellation::CancellationToken;
use std::mem::size_of;

mod validation;

pub(crate) const HARD_DEPTH: usize = 64;
pub(crate) const BYTE_CHUNK: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotLimits {
    pub max_bytes: usize,
    pub max_depth: usize,
    pub max_nodes: usize,
    pub max_sequence_len: usize,
    pub max_string_bytes: usize,
    pub max_total_string_bytes: usize,
    pub max_identifier_bytes: usize,
    pub max_definitions: usize,
    pub max_schema_nodes: usize,
    pub max_fields: usize,
    pub max_work: u64,
}

impl Default for SnapshotLimits {
    fn default() -> Self {
        Self {
            max_bytes: 32 * 1024 * 1024,
            max_depth: 64,
            max_nodes: 1_000_000,
            max_sequence_len: 100_000,
            max_string_bytes: 1024 * 1024,
            max_total_string_bytes: 16 * 1024 * 1024,
            max_identifier_bytes: 1024,
            max_definitions: 4096,
            max_schema_nodes: 65_536,
            max_fields: 256,
            max_work: 128_000_000,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct SnapshotOptions {
    pub limits: SnapshotLimits,
    pub cancellation: CancellationToken,
}

/// Per-operation accounting shared across validation, traversal and wire passes.
/// Counts are charged again for actual repeat passes, never reset to evade work.
pub(crate) struct Budget {
    pub(crate) limits: SnapshotLimits,
    pub(crate) stage: SnapshotStage,
    cancellation: CancellationToken,
    nodes: usize,
    schema_nodes: usize,
    allocated_nodes: usize,
    allocated_schema_nodes: usize,
    strings: usize,
    work: u64,
}

impl Budget {
    pub(crate) fn new(options: &SnapshotOptions, stage: SnapshotStage) -> Self {
        Self {
            limits: options.limits.clone(),
            stage,
            cancellation: options.cancellation.clone(),
            nodes: 0,
            schema_nodes: 0,
            allocated_nodes: 0,
            allocated_schema_nodes: 0,
            strings: 0,
            work: 0,
        }
    }

    pub(crate) fn error(
        &self,
        kind: SnapshotErrorKind,
        message: impl Into<String>,
    ) -> SnapshotError {
        SnapshotError::new(self.stage, kind, message)
    }

    pub(crate) fn cancel(&self) -> SnapshotResult<()> {
        self.cancellation
            .check()
            .map_err(|_| self.error(SnapshotErrorKind::Cancelled, "snapshot operation cancelled"))
    }

    pub(crate) fn cancellation(&self) -> &CancellationToken {
        &self.cancellation
    }

    pub(crate) fn work(&mut self, amount: u64) -> SnapshotResult<()> {
        self.cancel()?;
        self.work = self
            .work
            .checked_add(amount)
            .filter(|value| *value <= self.limits.max_work)
            .ok_or_else(|| SnapshotError::limit(self.stage, "work"))?;
        Ok(())
    }

    pub(crate) fn bytes(&mut self, length: usize) -> SnapshotResult<()> {
        for chunk in (0..length).step_by(BYTE_CHUNK) {
            self.work((length - chunk).min(BYTE_CHUNK) as u64)?;
        }
        self.cancel()
    }

    pub(crate) fn depth(&self, depth: usize) -> SnapshotResult<()> {
        self.cancel()?;
        self.count(depth, self.limits.max_depth.min(HARD_DEPTH), "depth")
    }

    pub(crate) fn node(&mut self, depth: usize) -> SnapshotResult<()> {
        self.depth(depth)?;
        self.work(1)?;
        self.nodes = self
            .nodes
            .checked_add(1)
            .filter(|value| *value <= self.limits.max_nodes)
            .ok_or_else(|| SnapshotError::limit(self.stage, "nodes"))?;
        Ok(())
    }

    pub(crate) fn schema_node(&mut self, depth: usize) -> SnapshotResult<()> {
        self.depth(depth)?;
        self.work(1)?;
        self.schema_nodes = self
            .schema_nodes
            .checked_add(1)
            .filter(|value| *value <= self.limits.max_schema_nodes)
            .ok_or_else(|| SnapshotError::limit(self.stage, "schema_nodes"))?;
        Ok(())
    }

    pub(crate) fn string(&mut self, value: &str, identifier: bool) -> SnapshotResult<()> {
        self.string_len(value.len(), identifier)
    }

    pub(crate) fn string_len(&mut self, length: usize, identifier: bool) -> SnapshotResult<()> {
        let (limit, name) = if identifier {
            (self.limits.max_identifier_bytes, "identifier_bytes")
        } else {
            (self.limits.max_string_bytes, "string_bytes")
        };
        self.count(length, limit, name)?;
        self.strings = self
            .strings
            .checked_add(length)
            .filter(|value| *value <= self.limits.max_total_string_bytes)
            .ok_or_else(|| SnapshotError::limit(self.stage, "total_string_bytes"))?;
        self.bytes(length)
    }

    pub(crate) fn copy_string(&mut self, value: &str, identifier: bool) -> SnapshotResult<String> {
        self.string(value, identifier)?;
        let mut output = String::new();
        output
            .try_reserve_exact(value.len())
            .map_err(|_| self.error(SnapshotErrorKind::Allocation, "string allocation failed"))?;
        let mut start = 0;
        while start < value.len() {
            self.cancel()?;
            let mut end = start.saturating_add(BYTE_CHUNK).min(value.len());
            while !value.is_char_boundary(end) {
                end -= 1;
            }
            output.push_str(&value[start..end]);
            start = end;
        }
        Ok(output)
    }

    pub(crate) fn count(
        &self,
        count: usize,
        limit: usize,
        name: &'static str,
    ) -> SnapshotResult<()> {
        self.cancel()?;
        if count > limit {
            Err(SnapshotError::limit(self.stage, name))
        } else {
            Ok(())
        }
    }

    /// Admit child counts before reservation, including zero-byte/unit children.
    pub(crate) fn children(&mut self, count: usize, schema: bool) -> SnapshotResult<()> {
        self.count(count, self.limits.max_sequence_len, "sequence_len")?;
        let (used, maximum, name) = if schema {
            (
                self.schema_nodes,
                self.limits.max_schema_nodes,
                "schema_nodes",
            )
        } else {
            (self.nodes, self.limits.max_nodes, "nodes")
        };
        self.count(count, maximum.saturating_sub(used), name)?;
        self.work(count as u64)
    }

    /// Charge aggregate decoded child slots before allocation. Pending siblings
    /// remain charged while descending; nested counts cannot each reuse the limit.
    pub(crate) fn admit_children(&mut self, count: usize, schema: bool) -> SnapshotResult<()> {
        self.children(count, schema)?;
        let (used, maximum, name) = if schema {
            (
                &mut self.allocated_schema_nodes,
                self.limits.max_schema_nodes,
                "schema_nodes",
            )
        } else {
            (&mut self.allocated_nodes, self.limits.max_nodes, "nodes")
        };
        *used = used
            .checked_add(count)
            .filter(|value| *value <= maximum)
            .ok_or_else(|| SnapshotError::limit(self.stage, name))?;
        Ok(())
    }

    pub(crate) fn reserve<T>(
        &mut self,
        values: &mut Vec<T>,
        additional: usize,
    ) -> SnapshotResult<()> {
        self.cancel()?;
        if values.capacity() - values.len() < additional {
            // Growth may move every existing element. Charge that conservative
            // cost and use amortized growth for incremental schema/path builders.
            let moved = values
                .len()
                .checked_mul(size_of::<T>())
                .ok_or_else(|| SnapshotError::limit(self.stage, "work"))?;
            self.bytes(moved)?;
            values.try_reserve(additional).map_err(|_| {
                self.error(SnapshotErrorKind::Allocation, "snapshot allocation failed")
            })?;
        }
        Ok(())
    }

    /// Start a new structural pass without resetting operation work accounting.
    pub(crate) fn reset_data_counts(&mut self) {
        self.nodes = 0;
        self.schema_nodes = 0;
        self.allocated_nodes = 0;
        self.allocated_schema_nodes = 0;
        self.strings = 0;
    }
}

pub(crate) fn versions_equal(
    a: &SchemaVersion,
    b: &SchemaVersion,
    budget: &mut Budget,
) -> SnapshotResult<bool> {
    validation::versions_equal(a, b, budget)
}

/// Compare already-validated schemas with charged, cancellable comparisons.
pub(crate) fn schemas_equal(
    a: &StateSchema,
    b: &StateSchema,
    budget: &mut Budget,
) -> SnapshotResult<bool> {
    validation::schemas_equal(a, b, budget)
}

pub(crate) fn validate_schema(schema: &StateSchema, budget: &mut Budget) -> SnapshotResult<()> {
    validation::schema(schema, budget)
}

pub(crate) fn validate_snapshot(
    header: &SnapshotHeader,
    schema: &StateSchema,
    payload: &StateValue,
    budget: &mut Budget,
) -> SnapshotResult<()> {
    validation::snapshot(header, schema, payload, budget)
}

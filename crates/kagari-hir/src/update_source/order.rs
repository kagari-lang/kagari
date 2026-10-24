//! Deterministic authored inventory order, independent of allocation order.

use crate::update_source::SourceEvidence;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMappingError, check_cancel},
        table::DefinitionTable,
    },
};
use std::cmp::Ordering;

fn sort<T>(
    values: &mut [T],
    mut compare: impl FnMut(&T, &T) -> Result<Ordering, DefinitionMappingError>,
) -> Result<(), DefinitionMappingError> {
    let mut failure = None;
    values.sort_by(|left, right| match compare(left, right) {
        Ok(order) => order,
        Err(error) => {
            failure = Some(error);
            Ordering::Equal
        }
    });
    failure.map_or(Ok(()), Err)
}

impl SourceEvidence {
    /// Order inventory by resolved identity; local facts retain HIR traversal order.
    pub fn canonical_order(
        &mut self,
        definitions: &DefinitionTable,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        sort(&mut self.bodies, |a, b| {
            check_cancel(cancel)?;
            Ok(definitions.compare(a.declaration, b.declaration)?)
        })?;
        sort(&mut self.initializers, |a, b| {
            check_cancel(cancel)?;
            Ok(definitions.compare(a.declaration, b.declaration)?)
        })
    }
}

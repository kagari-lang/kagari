//! Mandatory instruction provenance, independent of optional debugger records.
use crate::module::{FunctionMetadata, InstructionOrigin};

impl FunctionMetadata {
    /// Check the explicit choices for every many-to-one origin group.
    ///
    /// The ordinary verifier separately checks PC coverage and ordering. A group
    /// with one origin is already unambiguous; every larger group must select
    /// exactly one of its members. No debugger record supplies missing provenance.
    pub fn origin_selection_valid(&self) -> bool {
        let mut selected = self.instruction_origins.iter();
        let mut next = selected.next();
        let mut first = 0;
        for group in self.origins.chunk_by(|a, b| a.instruction == b.instruction) {
            let end = first + group.len();
            if group.len() > 1 {
                if next.is_none_or(|&id| id < first || id >= end) {
                    return false;
                }
                next = selected.next();
            }
            first = end;
        }
        next.is_none()
    }

    /// Resolve the actual executable instruction's origin, never an arbitrary alias.
    ///
    /// Returns `None` for an absent PC or missing/invalid disambiguation. Sealed
    /// products validate disambiguation before runtime preparation or native entry.
    pub fn instruction_origin(&self, pc: usize) -> Option<(usize, &InstructionOrigin)> {
        let first = self
            .origins
            .partition_point(|origin| origin.instruction < pc);
        let origin = self.origins.get(first)?;
        if origin.instruction != pc {
            return None;
        }
        if self
            .origins
            .get(first + 1)
            .is_none_or(|next| next.instruction != pc)
        {
            return Some((first, origin));
        }
        let selected = self
            .instruction_origins
            .binary_search_by_key(&pc, |&id| {
                self.origins
                    .get(id)
                    .map_or(usize::MAX, |origin| origin.instruction)
            })
            .ok()?;
        let id = self.instruction_origins[selected];
        let origin = self.origins.get(id)?;
        (origin.instruction == pc).then_some((id, origin))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kagari_common::span::Span;

    #[test]
    fn instruction_provenance_requires_an_explicit_choice_only_for_ambiguous_pcs() {
        let mut metadata = FunctionMetadata {
            origins: [0, 0, 1, 2, 2]
                .into_iter()
                .enumerate()
                .map(|(id, instruction)| InstructionOrigin {
                    instruction,
                    span: Span::new(id * 10, id * 10 + 2),
                    line: None,
                    column: None,
                })
                .collect(),
            instruction_origins: vec![1, 3],
            ..Default::default()
        };
        assert!(metadata.origin_selection_valid());
        for (pc, id) in [(0, 1), (1, 2), (2, 3)] {
            assert_eq!(metadata.instruction_origin(pc).unwrap().0, id);
        }
        for invalid in [
            vec![],
            vec![1],
            vec![1, 2, 3],
            vec![3, 1],
            vec![1, 5],
            vec![1, 3, 4],
        ] {
            metadata.instruction_origins = invalid;
            assert!(!metadata.origin_selection_valid());
        }
        metadata.instruction_origins.clear();
        assert!(metadata.instruction_origin(0).is_none());
        assert!(metadata.instruction_origin(3).is_none());
    }
}

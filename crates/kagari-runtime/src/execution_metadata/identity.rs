//! Semantic metadata identities never retain or reopen managed objects.
use kagari_gc::error::GcError;
use std::sync::atomic::{AtomicU64, Ordering};

/// Facts and cache keys use this nonce through distinct family-specific types.
/// It is deliberately unrelated to physical execution source coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct MetadataIdentity {
    owner: u64,
    sequence: u64,
}

/// One Runtime supplies identities for every executable metadata family. There
/// is no record directory, recycling list or weak allocation backing.
#[derive(Debug)]
pub(crate) struct MetadataIds {
    owner: u64,
    next: Option<u64>,
}

impl Default for MetadataIds {
    fn default() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self {
            owner: NEXT
                .try_update(Ordering::Relaxed, Ordering::Relaxed, |owner| {
                    owner.checked_add(1)
                })
                .expect("executable metadata identity domains"),
            next: Some(0),
        }
    }
}

impl MetadataIds {
    pub(crate) fn fresh(&mut self) -> Result<MetadataIdentity, GcError> {
        let sequence = self
            .next
            .ok_or(GcError::Limit("executable metadata identities"))?;
        self.next = sequence.checked_add(1);
        Ok(MetadataIdentity {
            owner: self.owner,
            sequence,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_domains_and_sequences_never_alias_or_wrap() {
        let mut first = MetadataIds::default();
        let mut second = MetadataIds::default();
        let old = first.fresh().unwrap();
        let next = first.fresh().unwrap();
        let foreign = second.fresh().unwrap();
        assert_ne!(old, next);
        assert_ne!(old.owner, foreign.owner);
        first.next = Some(u64::MAX);
        let last = first.fresh().unwrap();
        assert_ne!(last, old);
        assert_ne!(last, next);
        assert!(matches!(
            first.fresh(),
            Err(GcError::Limit("executable metadata identities"))
        ));
    }
}

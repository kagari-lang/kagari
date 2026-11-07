//! Exact duplicate merging keeps differing context interpretations distinguishable.
use crate::session::publications::{Contribution, ContributionOwner};
use lsp_types::Diagnostic;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    io::{self, Error},
    sync::Arc,
};

pub(super) fn diagnostics(
    contributions: &BTreeMap<ContributionOwner, Arc<Contribution>>,
    uri: &str,
    limit: usize,
) -> io::Result<Vec<Diagnostic>> {
    let mut output: Vec<(Diagnostic, BTreeSet<&str>, BTreeSet<&ContributionOwner>)> = Vec::new();
    let mut hashes: HashMap<u64, Vec<usize>> = HashMap::new();
    let mut bytes = 0usize;
    for (owner, contribution) in contributions {
        let Some(group) = contribution.groups.get(uri) else {
            continue;
        };
        for record in group.records() {
            let candidates = hashes.entry(record.fingerprint()).or_default();
            if let Some(index) = candidates
                .iter()
                .copied()
                .find(|index| output[*index].0 == *record.diagnostic())
            {
                output[index].1.insert(&contribution.label);
                output[index].2.insert(owner);
            } else {
                charge(&mut bytes, record.retained_bytes(), limit)?;
                candidates.push(output.len());
                output.push((
                    record.diagnostic().clone(),
                    BTreeSet::from([contribution.label.as_str()]),
                    BTreeSet::from([owner]),
                ));
            }
        }
    }
    let mut sites = BTreeMap::<_, Vec<usize>>::new();
    for (index, (diagnostic, _, _)) in output.iter().enumerate() {
        let range = diagnostic.range;
        sites
            .entry((
                range.start.line,
                range.start.character,
                range.end.line,
                range.end.character,
            ))
            .or_default()
            .push(index);
    }
    let mut divergent = vec![false; output.len()];
    for indices in sites.values() {
        let owner_sets: BTreeSet<_> = indices.iter().map(|index| &output[*index].2).collect();
        if owner_sets.len() > 1 {
            for index in indices {
                divergent[*index] = true;
            }
        }
    }
    let mut diagnostics = Vec::with_capacity(output.len());
    for ((mut diagnostic, labels, _), divergent) in output.into_iter().zip(divergent) {
        if divergent {
            let additional =
                "\nContexts: ".len() + labels.iter().map(|label| label.len() + 2).sum::<usize>();
            charge(&mut bytes, additional, limit)?;
            diagnostic.message.reserve_exact(additional);
            diagnostic.message.push_str("\nContexts: ");
            for (index, label) in labels.into_iter().enumerate() {
                if index > 0 {
                    diagnostic.message.push_str(", ");
                }
                diagnostic.message.push_str(label);
            }
        }
        diagnostics.push(diagnostic);
    }
    Ok(diagnostics)
}

fn charge(bytes: &mut usize, additional: usize, limit: usize) -> io::Result<()> {
    if additional > limit.saturating_sub(*bytes) {
        return Err(Error::other("diagnostic merge exceeds frame budget"));
    }
    *bytes += additional;
    Ok(())
}

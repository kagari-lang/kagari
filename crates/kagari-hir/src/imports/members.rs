//! Source namespaces include enum variants without a standard-library name table.

use crate::{
    hir::ExportItem,
    imports::{
        ImportTarget, SourceCatalog, SourceCatalogEntry, SourceImport, canonical_namespace_target,
    },
};
use kagari_common::identity::ModuleIdentity;

impl SourceImport {
    pub(crate) fn is_namespace(&self) -> bool {
        matches!(self.item, None | Some(ExportItem::Enum(_)))
    }

    /// Module paths retain their qualification; an imported enum exposes its own
    /// variants relative to the enum name. Module globs select only immediate names.
    pub(crate) fn namespace_members(&self) -> impl Iterator<Item = (&str, &[ExportItem])> {
        let owner = self.item.and_then(|item| {
            self.members
                .iter()
                .find_map(|(name, items)| (items.as_slice() == [item]).then_some(name.as_str()))
        });
        self.members
            .iter()
            .filter_map(move |(name, items)| match self.item {
                None => Some((name.as_str(), items.as_slice())),
                Some(ExportItem::Enum(_)) => {
                    let (enumeration, member) = name.split_once("::")?;
                    (Some(enumeration) == owner).then_some((member, items.as_slice()))
                }
                _ => None,
            })
    }
}

impl SourceCatalog<'_> {
    /// Follow namespace re-exports before selecting an associated source member.
    /// Each iteration consumes a path component; re-export cycles are rejected by
    /// canonical_namespace_target rather than recursing through source spellings.
    pub(super) fn member_path(
        &self,
        entry: &SourceCatalogEntry<'_>,
        mut path: &str,
        importer: &ModuleIdentity,
    ) -> Option<ImportTarget> {
        let mut source = entry.target(None, importer);
        loop {
            let direct = source
                .namespace_members()
                .find(|(name, _)| *name == path)
                .and_then(|(_, items)| match items {
                    [item] => Some(*item),
                    _ => None,
                });
            if let Some(item) = direct {
                source.item = Some(item);
                return Some(ImportTarget::Source(source));
            }
            let (prefix, suffix) = path.split_once("::")?;
            let item = source
                .namespace_members()
                .find(|(name, _)| *name == prefix)
                .and_then(|(_, items)| match items {
                    [item] => Some(*item),
                    _ => None,
                })?;
            source.item = Some(item);
            let ImportTarget::Source(next) =
                canonical_namespace_target(ImportTarget::Source(source), self, importer)?
            else {
                return None;
            };
            source = next;
            path = suffix;
        }
    }
}

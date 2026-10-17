use crate::{
    error::PackageError,
    modules::{SourceInput, VirtualSources},
    path,
};
use kagari_common::cancellation::CancellationToken;
use std::collections::BTreeSet;

/// Directory discovery within the same approved root used for source reads.
pub trait InputTree: SourceInput {
    /// Returns immediate directory names, in deterministic order. Missing directories
    /// produce an empty list. Access errors and exhausted budgets remain errors.
    fn directories(
        &self,
        directory: &str,
        max_entries: usize,
        cancel: &CancellationToken,
    ) -> Result<Vec<String>, PackageError>;
}

impl InputTree for VirtualSources {
    fn directories(
        &self,
        directory: &str,
        max_entries: usize,
        cancel: &CancellationToken,
    ) -> Result<Vec<String>, PackageError> {
        let directory = path::join(
            "",
            if directory.is_empty() { "." } else { directory },
            directory,
        )?;
        let prefix = if directory.is_empty() {
            String::new()
        } else {
            format!("{directory}/")
        };
        let mut children = BTreeSet::new();
        for name in self.files.keys() {
            cancel.check()?;
            if let Some(rest) = name.strip_prefix(&prefix)
                && let Some((child, _)) = rest.split_once('/')
            {
                children.insert(child.to_owned());
                if children.len() > max_entries {
                    return Err(PackageError::input(
                        &directory,
                        "directory discovery exceeds limit",
                    ));
                }
            }
        }
        cancel.check()?;
        Ok(children.into_iter().collect())
    }
}

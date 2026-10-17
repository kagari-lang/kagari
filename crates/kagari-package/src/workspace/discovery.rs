use crate::{error::PackageError, input::InputTree, path, workspace::WorkspaceLimits};
use glob::{MatchOptions, Pattern};
use kagari_common::cancellation::CancellationToken;
use std::collections::BTreeSet;

pub(super) struct Discovery<'a, T> {
    pub input: &'a T,
    pub limits: WorkspaceLimits,
    pub cancel: &'a CancellationToken,
    pub visited: usize,
}

impl<T: InputTree> Discovery<'_, T> {
    pub fn expand(
        &mut self,
        base: &str,
        patterns: &[String],
        origin: &str,
    ) -> Result<BTreeSet<String>, PackageError> {
        let mut result = BTreeSet::new();
        for spelling in patterns {
            self.cancel.check()?;
            let normalized = path::join(base, spelling, origin)?;
            let pattern = parse(&normalized, origin)?;
            let segments = normalized.split('/').collect::<Vec<_>>();
            let first_glob = segments
                .iter()
                .position(|segment| segment.contains(['*', '?', '[']));
            let Some(first_glob) = first_glob else {
                result.insert(normalized);
                continue;
            };
            let prefix = segments[..first_glob].join("/");
            let recursive = segments.contains(&"**");
            let max_depth = if recursive {
                self.limits.max_depth
            } else {
                segments.len() - first_glob
            };
            let mut pending = vec![(prefix, 0)];
            while let Some((directory, depth)) = pending.pop() {
                self.cancel.check()?;
                if self.visited >= self.limits.max_directories {
                    return Err(PackageError::input(
                        origin,
                        "workspace directory discovery exceeds limit",
                    ));
                }
                self.visited += 1;
                if pattern.matches_with(&directory, options()) {
                    result.insert(directory.clone());
                }
                if depth >= max_depth && !recursive {
                    continue;
                }
                let children = self.input.directories(
                    &directory,
                    self.limits.max_directories - self.visited,
                    self.cancel,
                )?;
                if depth >= self.limits.max_depth && !children.is_empty() {
                    return Err(PackageError::input(
                        origin,
                        "workspace directory depth exceeds limit",
                    ));
                }
                for name in children.into_iter().rev() {
                    pending.push((path::join(&directory, &name, origin)?, depth + 1));
                }
            }
        }
        Ok(result)
    }
}

pub(super) fn exclusions(
    base: &str,
    spellings: &[String],
    origin: &str,
) -> Result<Vec<Pattern>, PackageError> {
    spellings
        .iter()
        .map(|value| parse(&path::join(base, value, origin)?, origin))
        .collect()
}

pub(super) fn is_excluded(directory: &str, exclusions: &[Pattern]) -> bool {
    let mut cursor = directory;
    loop {
        if exclusions
            .iter()
            .any(|pattern| pattern.matches_with(cursor, options()))
        {
            return true;
        }
        if cursor.is_empty() {
            return false;
        }
        cursor = path::parent(cursor);
    }
}

fn parse(pattern: &str, origin: &str) -> Result<Pattern, PackageError> {
    Pattern::new(pattern)
        .map_err(|error| PackageError::input(origin, format!("invalid workspace pattern: {error}")))
}

fn options() -> MatchOptions {
    MatchOptions {
        case_sensitive: true,
        require_literal_separator: true,
        require_literal_leading_dot: true,
    }
}

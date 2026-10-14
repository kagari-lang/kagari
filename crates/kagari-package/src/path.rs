use crate::error::PackageError;

/// Normalizes a portable relative path, rejecting absolute paths and root escapes.
///
/// `base` and the result are slash-separated paths within one approved input root.
/// Backslashes and drive/URI prefixes are rejected on every platform. Filesystem
/// callers must additionally canonicalize and enforce their symlink policy.
pub fn join(base: &str, path: &str, origin: &str) -> Result<String, PackageError> {
    if path.is_empty()
        || [base, path]
            .iter()
            .any(|value| value.starts_with('/') || value.contains(['\\', ':', '\0']))
    {
        return Err(PackageError::input(
            origin,
            "expected a portable relative path",
        ));
    }
    let mut parts = Vec::new();
    for part in base.split('/').chain(path.split('/')) {
        match part {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return Err(PackageError::input(origin, "path escapes the input root"));
                }
            }
            part => parts.push(part),
        }
    }
    Ok(parts.join("/"))
}

pub(crate) fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(parent, _)| parent)
}

/// Computes a portable locator between two already normalized input directories.
pub(crate) fn relative(from: &str, to: &str) -> String {
    let from = from
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let to = to
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    let mut parts = vec![".."; from.len() - common];
    parts.extend_from_slice(&to[common..]);
    parts.join("/")
}

/// Normalizes a logical locator; leading `..` denotes explicit external membership.
/// This never authorizes IO outside the approved input root.
pub(crate) fn locator(value: &str, origin: &str) -> Result<String, PackageError> {
    if value.starts_with('/') || value.contains(['\\', ':', '\0']) {
        return Err(PackageError::input(
            origin,
            "expected a portable package locator",
        ));
    }
    let mut parts = Vec::new();
    for part in value.split('/') {
        match part {
            "" | "." => {}
            ".." if parts.last().is_some_and(|previous| *previous != "..") => {
                parts.pop();
            }
            part => parts.push(part),
        }
    }
    Ok(parts.join("/"))
}

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

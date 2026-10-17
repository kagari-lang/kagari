use kagari_source::source_database::normalize_source_name;
use lsp_types::Uri;

/// Validate a document URI and return its source-owned normalized key.
///
/// File URIs require an absolute local path without credentials, ports, queries or
/// fragments. Source normalization owns decoding, path folding and decoded-path
/// validation. Non-file identities retain all scheme-specific bytes unchanged.
pub(crate) fn source_name(uri: &Uri) -> Result<String, String> {
    // `Uri::is_absolute()` excludes fragments, which virtual document keys retain.
    let scheme = uri
        .scheme()
        .ok_or("document URI requires an absolute scheme")?
        .as_str();
    if scheme.len() == 1 {
        return Err("single-letter document URI schemes are reserved for Windows drives".into());
    }
    if scheme.eq_ignore_ascii_case("file") {
        if uri.query().is_some() || uri.fragment().is_some() {
            return Err("file document URIs cannot contain a query or fragment".into());
        }
        if let Some(authority) = uri.authority() {
            if authority.userinfo().is_some() || authority.port().is_some() {
                return Err("file document URIs cannot contain credentials or ports".into());
            }
            let host = authority.host().as_str();
            if !host.is_empty() && !host.eq_ignore_ascii_case("localhost") {
                return Err("nonlocal file document URI authorities are unsupported".into());
            }
        }
        let path = uri.path();
        if !path.is_absolute() {
            return Err("file document URI requires an absolute filesystem path".into());
        }
        if path.as_str().starts_with("//") {
            return Err("UNC document paths are unsupported".into());
        }
    }
    normalize_source_name(uri.as_str())
}

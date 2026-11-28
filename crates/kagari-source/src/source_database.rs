mod generated;
#[cfg(test)]
mod generated_tests;

use self::generated::GeneratedDocuments;
use crate::{
    identity::{FileId, FileSpan, Revision},
    package::PackageEnvironment,
    source::SourceFile,
};
use kagari_common::{cancellation::CancellationToken, identity::ModuleIdentity};
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs, io,
    sync::Arc,
};
use thiserror::Error;

/// A source name could not be normalized to a filesystem path or virtual source key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum SourceNameError {
    #[error("source name contains NUL")]
    ContainsNul,
    #[error("UNC source paths are unsupported")]
    UnsupportedUncPath,
    #[error("drive-relative source paths are unsupported")]
    UnsupportedDriveRelativePath,
    #[error("empty source name")]
    Empty,
    #[error("remote file URI authorities are unsupported")]
    UnsupportedRemoteFileAuthority,
    #[error("file URI requires an absolute filesystem path")]
    FileUriRequiresAbsolutePath,
    #[error("invalid URI escape")]
    InvalidUriEscape,
    #[error("source URI is not UTF-8")]
    InvalidUriUtf8,
}

/// A source database operation rejected its input, failed to read a file, or was cancelled.
#[derive(Debug, Error)]
pub enum SourceDatabaseError {
    #[error(transparent)]
    Name(#[from] SourceNameError),
    #[error("source root must be an absolute filesystem path")]
    RootRequiresAbsolutePath,
    #[error("virtual sources must be supplied by the host")]
    VirtualSourceRequiresHost,
    #[error("{path}: {source}")]
    ReadFile {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("source capture cancelled")]
    CaptureCancelled,
    #[error("duplicate captured source key or module identity")]
    DuplicateCapturedSource,
    #[error("source module identity is already bound to another source")]
    SourceModuleAlreadyBound,
    #[error("module `{module}` is already bound to another source")]
    ModuleAlreadyBound { module: ModuleIdentity },
    #[error("module identity path segment limit exceeded")]
    ModulePathLimitExceeded,
    #[error("module identity requires a package and nonempty path components")]
    InvalidModuleIdentity,
    #[error("generated source `{name}` is read-only")]
    GeneratedReadOnly { name: String },
    #[error("generated source key `{key}` conflicts with another source")]
    GeneratedKeyConflict { key: String },
    #[error("generated display name `{display}` conflicts with another source")]
    GeneratedDisplayNameConflict { display: String },
    #[error("generated source key `{key}` belongs to a different module")]
    GeneratedKeyModuleMismatch { key: String },
}

#[derive(Debug, Clone, Copy)]
pub enum SourceLayer {
    Base,
    Overlay,
}

#[derive(Debug, Clone)]
struct Document {
    id: FileId,
    module: ModuleIdentity,
    base: Option<String>,
    overlay: Option<String>,
    display_name: String,
}

/// One captured document: stable database key, diagnostic origin, module and bytes.
pub struct CapturedSource {
    pub key: String,
    pub display_name: String,
    pub module: ModuleIdentity,
    pub text: String,
}

#[derive(Debug, Clone, Default)]
pub struct SourceSnapshot {
    revision: Revision,
    files: Arc<BTreeMap<FileId, Arc<SourceFile>>>,
    packages: Arc<PackageEnvironment>,
}

impl SourceSnapshot {
    /// Analyze an existing file without allocating a new source or module identity.
    pub fn single_file(file: Arc<SourceFile>) -> Self {
        Self {
            revision: file.revision(),
            files: Arc::new([(file.id(), file)].into()),
            packages: Arc::default(),
        }
    }

    pub fn revision(&self) -> Revision {
        self.revision
    }

    pub fn packages(&self) -> &PackageEnvironment {
        &self.packages
    }

    pub fn file(&self, file: FileId) -> Option<&Arc<SourceFile>> {
        self.files.get(&file)
    }

    pub fn files(&self) -> impl Iterator<Item = &Arc<SourceFile>> {
        self.files.values()
    }

    pub fn module(&self, identity: &ModuleIdentity) -> Option<&Arc<SourceFile>> {
        self.files
            .values()
            .find(|file| file.module_identity() == identity)
    }

    pub fn contains(&self, span: FileSpan) -> bool {
        self.file(span.file)
            .is_some_and(|file| file.revision() == span.revision && file.span(span.range).is_some())
    }
}

#[derive(Debug)]
pub struct SourceDatabase {
    root: String,
    documents: BTreeMap<String, Document>,
    generated: GeneratedDocuments,
    snapshot: SourceSnapshot,
}

impl Default for SourceDatabase {
    fn default() -> Self {
        Self::new(
            env::current_dir()
                .expect("working directory unavailable")
                .to_string_lossy()
                .as_ref(),
        )
        .map_err(|error| error.to_string())
        .expect("working directory is not a valid source root")
    }
}

impl SourceDatabase {
    /// Capture an absolute project root once; later working-directory changes
    /// cannot change the meaning of relative source names in this database.
    pub fn new(root: &str) -> Result<Self, SourceDatabaseError> {
        let root = normalize_source_name(root)?;
        if !is_absolute_path(&root) {
            return Err(SourceDatabaseError::RootRequiresAbsolutePath);
        }
        Ok(Self {
            root,
            documents: BTreeMap::new(),
            generated: GeneratedDocuments::default(),
            snapshot: SourceSnapshot::default(),
        })
    }

    /// The normalized absolute root captured at construction; accessing it performs no IO.
    pub fn root(&self) -> &str {
        &self.root
    }

    /// Resolve a source key against this database's root without accessing the filesystem.
    pub fn source_name(&self, name: &str) -> Result<String, SourceNameError> {
        // Classify before folding: `./untitled:Name` is a relative filesystem path.
        let virtual_source =
            source_uri_scheme(name).is_some_and(|scheme| !scheme.eq_ignore_ascii_case("file"));
        let name = normalize_source_name(name)?;
        if is_absolute_path(&name) || virtual_source {
            Ok(name)
        } else {
            normalize_source_name(&format!("{}/{name}", self.root))
        }
    }

    pub fn load_file(&mut self, path: &str) -> Result<FileId, SourceDatabaseError> {
        let name = self.writable_name(path)?;
        if source_uri_scheme(&name).is_some() {
            return Err(SourceDatabaseError::VirtualSourceRequiresHost);
        }
        let text = match fs::read_to_string(&name) {
            Ok(text) => text,
            Err(source) => return Err(SourceDatabaseError::ReadFile { path: name, source }),
        };
        self.set(&name, text, SourceLayer::Base)
    }

    pub fn snapshot(&self) -> SourceSnapshot {
        self.snapshot.clone()
    }

    /// Atomically replaces the complete input set while retaining unchanged file IDs/revisions.
    /// Captured text replaces base/overlay layers; documents outside the batch are removed.
    /// Invalid input or cancellation leaves the database and retained snapshots unchanged.
    pub fn replace_captured(
        &mut self,
        sources: Vec<CapturedSource>,
        packages: PackageEnvironment,
        cancel: &CancellationToken,
    ) -> Result<SourceSnapshot, SourceDatabaseError> {
        cancel
            .check()
            .map_err(|_| SourceDatabaseError::CaptureCancelled)?;
        let revision = self.next_revision();
        let mut documents = BTreeMap::new();
        let mut files = BTreeMap::new();
        let mut identities = BTreeSet::new();
        for source in sources {
            cancel
                .check()
                .map_err(|_| SourceDatabaseError::CaptureCancelled)?;
            let key = self.source_name(&source.key)?;
            validate_module(&source.module)?;
            if !identities.insert(source.module.clone()) || documents.contains_key(&key) {
                return Err(SourceDatabaseError::DuplicateCapturedSource);
            }
            let id = self
                .documents
                .get(&key)
                .map(|document| document.id)
                .or_else(|| self.generated.keys.get(&key).copied())
                .unwrap_or_else(FileId::fresh);
            let file = self.captured_file(id, revision, &source);
            files.insert(id, file);
            documents.insert(
                key,
                Document {
                    id,
                    module: source.module,
                    base: Some(source.text),
                    overlay: None,
                    display_name: source.display_name,
                },
            );
        }
        let unchanged = *self.snapshot.packages == packages
            && self.snapshot.files.len() == files.len()
            && files.iter().all(|(id, file)| {
                self.snapshot
                    .files
                    .get(id)
                    .is_some_and(|old| Arc::ptr_eq(old, file))
            });
        cancel
            .check()
            .map_err(|_| SourceDatabaseError::CaptureCancelled)?;
        self.documents = documents;
        self.generated = GeneratedDocuments::default();
        self.snapshot = SourceSnapshot {
            revision: if unchanged {
                self.snapshot.revision
            } else {
                revision
            },
            files: Arc::new(files),
            packages: Arc::new(packages),
        };
        Ok(self.snapshot())
    }

    pub fn set(
        &mut self,
        name: &str,
        text: String,
        layer: SourceLayer,
    ) -> Result<FileId, SourceDatabaseError> {
        let name = self.writable_name(name)?;
        if !self.documents.contains_key(&name)
            && self.module_bound_elsewhere(&name, &ModuleIdentity::single_file(name.clone()))
        {
            return Err(SourceDatabaseError::SourceModuleAlreadyBound);
        }
        let document = self
            .documents
            .entry(name.clone())
            .or_insert_with(|| Document {
                id: FileId::fresh(),
                module: ModuleIdentity::single_file(name.clone()),
                base: None,
                overlay: None,
                display_name: name.clone(),
            });
        match layer {
            SourceLayer::Base => document.base = Some(text),
            SourceLayer::Overlay => document.overlay = Some(text),
        }
        let id = document.id;
        self.refresh(&name);
        Ok(id)
    }

    pub fn close_overlay(&mut self, name: &str) -> Result<(), SourceDatabaseError> {
        let name = self.writable_name(name)?;
        if let Some(document) = self.documents.get_mut(&name) {
            document.overlay = None;
        }
        self.refresh(&name);
        Ok(())
    }

    /// Bind a logical package/module before analysis. An overlay shares this binding.
    pub fn bind_module(
        &mut self,
        name: &str,
        module: ModuleIdentity,
    ) -> Result<FileId, SourceDatabaseError> {
        let name = self.writable_name(name)?;
        validate_module(&module)?;
        if self.module_bound_elsewhere(&name, &module) {
            return Err(SourceDatabaseError::ModuleAlreadyBound { module });
        }
        let document = self
            .documents
            .entry(name.clone())
            .or_insert_with(|| Document {
                id: FileId::fresh(),
                module: module.clone(),
                base: None,
                overlay: None,
                display_name: name.clone(),
            });
        document.module = module;
        let id = document.id;
        self.refresh(&name);
        Ok(id)
    }

    pub fn file_id(&self, name: &str) -> Option<FileId> {
        let name = self.source_name(name).ok()?;
        self.documents
            .get(&name)
            .map(|doc| doc.id)
            .or_else(|| self.generated.lookup(&name))
    }

    fn writable_name(&self, name: &str) -> Result<String, SourceDatabaseError> {
        let name = self.source_name(name)?;
        if self.generated.lookup(&name).is_some() {
            return Err(SourceDatabaseError::GeneratedReadOnly { name });
        }
        Ok(name)
    }

    fn module_bound_elsewhere(&self, key: &str, module: &ModuleIdentity) -> bool {
        self.documents
            .iter()
            .any(|(other, document)| other != key && &document.module == module)
            || self
                .generated
                .keys
                .values()
                .any(|id| self.snapshot.files[id].module_identity() == module)
    }

    fn next_revision(&self) -> Revision {
        Revision(
            self.snapshot
                .revision
                .0
                .checked_add(1)
                .expect("source revision exhausted"),
        )
    }

    fn captured_file(
        &self,
        id: FileId,
        revision: Revision,
        source: &CapturedSource,
    ) -> Arc<SourceFile> {
        self.snapshot
            .file(id)
            .filter(|file| {
                file.text() == source.text
                    && file.module_identity() == &source.module
                    && file.name() == source.display_name
            })
            .cloned()
            .unwrap_or_else(|| {
                Arc::new(
                    SourceFile::new(&source.display_name, &source.text).with_identity(
                        id,
                        revision,
                        source.module.clone(),
                    ),
                )
            })
    }

    fn refresh(&mut self, name: &str) {
        let Some(document) = self.documents.get(name) else {
            return;
        };
        let text = document.overlay.as_ref().or(document.base.as_ref());
        if self
            .snapshot
            .files
            .get(&document.id)
            .map(|file| (file.text(), file.module_identity()))
            == text.map(|text| (text.as_str(), &document.module))
        {
            return;
        }
        self.snapshot.revision = self.next_revision();
        let files = Arc::make_mut(&mut self.snapshot.files);
        if let Some(text) = text {
            files.insert(
                document.id,
                Arc::new(SourceFile::new(&document.display_name, text).with_identity(
                    document.id,
                    self.snapshot.revision,
                    document.module.clone(),
                )),
            );
        } else {
            files.remove(&document.id);
        }
    }
}

fn validate_module(module: &ModuleIdentity) -> Result<(), SourceDatabaseError> {
    if !module.within_path_limit() {
        return Err(SourceDatabaseError::ModulePathLimitExceeded);
    }
    if module.package.0.is_empty() || module.path.iter().any(String::is_empty) {
        return Err(SourceDatabaseError::InvalidModuleIdentity);
    }
    Ok(())
}

/// Lexically normalize a filesystem path or source URI without requiring it to exist.
///
/// Non-file URIs normalize only the scheme's case; their remaining bytes are opaque.
/// Single-letter prefixes such as `c:` are reserved for Windows drives, including
/// ambiguous drive-relative forms, which are rejected. To name a relative path with
/// a URI-like first component, use an explicit `./` prefix when passing it to a database.
/// Local file URIs decode to filesystem paths; UNC paths and remote authorities fail.
pub fn normalize_source_name(name: &str) -> Result<String, SourceNameError> {
    if name.contains('\0') {
        return Err(SourceNameError::ContainsNul);
    }
    let mut path = if let Some(scheme) = source_uri_scheme(name) {
        let rest = &name[scheme.len() + 1..];
        if !scheme.eq_ignore_ascii_case("file") {
            return Ok(format!("{}:{rest}", scheme.to_ascii_lowercase()));
        }
        local_file_uri_path(rest)?
    } else {
        name.to_owned()
    }
    .replace('\\', "/");
    if path.starts_with("//") {
        return Err(SourceNameError::UnsupportedUncPath);
    }
    if path.contains('\0') {
        return Err(SourceNameError::ContainsNul);
    }
    if path.starts_with('/') && path.as_bytes().get(2) == Some(&b':') {
        path.remove(0);
    }
    let drive = path.as_bytes().get(1) == Some(&b':');
    if drive {
        if !path.as_bytes()[0].is_ascii_alphabetic() || path.as_bytes().get(2) != Some(&b'/') {
            return Err(SourceNameError::UnsupportedDriveRelativePath);
        }
        path.replace_range(..1, &path[..1].to_ascii_lowercase());
    }
    let absolute = path.starts_with('/');
    let mut components = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if components
                    .last()
                    .is_some_and(|p: &&str| *p != ".." && !p.ends_with(':'))
                {
                    components.pop();
                } else if !absolute && !drive {
                    components.push(part);
                }
            }
            _ => components.push(part),
        }
    }
    let normalized = format!(
        "{}{}",
        if absolute { "/" } else { "" },
        components.join("/")
    );
    if normalized.is_empty() {
        return Err(SourceNameError::Empty);
    }
    Ok(normalized)
}

/// Recognize RFC 3986 schemes, reserving single-letter prefixes for Windows paths.
/// This is source-key classification, not protocol-level URI validation.
fn source_uri_scheme(name: &str) -> Option<&str> {
    let (scheme, _) = name.split_once(':')?;
    (scheme.len() > 1
        && scheme.as_bytes()[0].is_ascii_alphabetic()
        && scheme
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.')))
    .then_some(scheme)
}

fn local_file_uri_path(uri: &str) -> Result<String, SourceNameError> {
    let path = if let Some(uri) = uri.strip_prefix("//") {
        let (authority, path) = uri.split_at(uri.find('/').unwrap_or(uri.len()));
        if !authority.is_empty() && !authority.eq_ignore_ascii_case("localhost") {
            return Err(SourceNameError::UnsupportedRemoteFileAuthority);
        }
        path
    } else {
        uri
    };
    if !path.starts_with('/') {
        return Err(SourceNameError::FileUriRequiresAbsolutePath);
    }
    let mut bytes = Vec::new();
    let input = path.as_bytes();
    let mut cursor = 0;
    while cursor < input.len() {
        if input[cursor] == b'%' {
            let digits = input
                .get(cursor + 1..cursor + 3)
                .ok_or(SourceNameError::InvalidUriEscape)?;
            let digits = str::from_utf8(digits).map_err(|_| SourceNameError::InvalidUriEscape)?;
            bytes.push(
                u8::from_str_radix(digits, 16).map_err(|_| SourceNameError::InvalidUriEscape)?,
            );
            cursor += 3;
        } else {
            bytes.push(input[cursor]);
            cursor += 1;
        }
    }
    String::from_utf8(bytes).map_err(|_| SourceNameError::InvalidUriUtf8)
}

fn is_absolute_path(path: &str) -> bool {
    path.starts_with('/')
        || (path.as_bytes().get(1) == Some(&b':') && path.as_bytes().get(2) == Some(&b'/'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::line_index::{Position, PositionEncoding};
    use kagari_common::span::Span;
    use std::error::Error as _;

    #[test]
    fn captured_batches_preserve_previous_inputs_on_invalid_or_cancelled_replacement() {
        let mut db = SourceDatabase::default();
        let id = db
            .set("retained.kgr", "before".into(), SourceLayer::Base)
            .unwrap();
        let old = db.snapshot();
        let capture = || CapturedSource {
            key: "mem://new".into(),
            display_name: "src/lib.kgr".into(),
            module: ModuleIdentity::single_file("new"),
            text: "after".into(),
        };
        assert!(matches!(
            db.replace_captured(
                vec![capture(), capture()],
                PackageEnvironment::default(),
                &CancellationToken::default(),
            ),
            Err(SourceDatabaseError::DuplicateCapturedSource)
        ));
        let cancel = CancellationToken::default();
        cancel.cancel();
        let error = db
            .replace_captured(
                vec![capture(), capture()],
                PackageEnvironment::default(),
                &cancel,
            )
            .unwrap_err();
        assert!(matches!(error, SourceDatabaseError::CaptureCancelled));
        assert_eq!(error.to_string(), "source capture cancelled");
        assert_eq!(db.snapshot().revision(), old.revision());
        assert_eq!(db.file_id("retained.kgr"), Some(id));
        assert!(db.file_id("mem://new").is_none());
        assert_eq!(db.snapshot().file(id).unwrap().text(), "before");
        let next = db
            .replace_captured(
                vec![capture()],
                PackageEnvironment::default(),
                &CancellationToken::default(),
            )
            .unwrap();
        assert!(next.file(id).is_none());
        assert_eq!(next.files().next().unwrap().name(), "src/lib.kgr");
        assert_eq!(old.file(id).unwrap().text(), "before");
        let unchanged = db
            .replace_captured(
                vec![capture()],
                PackageEnvironment::default(),
                &CancellationToken::default(),
            )
            .unwrap();
        assert_eq!(unchanged.revision(), next.revision());
    }

    #[test]
    fn single_file_snapshot_preserves_identity_revision_and_span_validation() {
        let mut db = SourceDatabase::default();
        let id = db
            .set("existing.kgr", "before".into(), SourceLayer::Base)
            .unwrap();
        let old = db.snapshot().file(id).unwrap().clone();
        db.set("existing.kgr", "after".into(), SourceLayer::Overlay)
            .unwrap();
        let file = db.snapshot().file(id).unwrap().clone();
        let snapshot = SourceSnapshot::single_file(file.clone());
        assert_eq!(snapshot.revision(), file.revision());
        assert!(Arc::ptr_eq(snapshot.file(id).unwrap(), &file));
        assert!(Arc::ptr_eq(
            snapshot.module(file.module_identity()).unwrap(),
            &file
        ));
        assert!(snapshot.contains(file.span(Span::new(0, 5)).unwrap()));
        assert!(!snapshot.contains(old.span(Span::new(0, 5)).unwrap()));
        assert_eq!(snapshot.files().count(), 1);
    }

    #[test]
    fn logical_module_bindings_survive_overlays_and_invalidate_old_revisions() {
        use kagari_common::identity::{ModuleIdentity, PackageId};
        let mut db = SourceDatabase::new("C:/project").unwrap();
        let identity = ModuleIdentity {
            package: PackageId("game".into()),
            path: vec!["combat".into()],
        };
        let id = db.bind_module("src/combat.kgr", identity.clone()).unwrap();
        assert_eq!(
            db.set("src/combat.kgr", "base".into(), SourceLayer::Base)
                .unwrap(),
            id
        );
        let old = db.snapshot();
        db.set("src/combat.kgr", "overlay".into(), SourceLayer::Overlay)
            .unwrap();
        assert_eq!(db.snapshot().module(&identity).unwrap().text(), "overlay");
        assert!(matches!(
            db.bind_module("different.kgr", identity.clone()),
            Err(SourceDatabaseError::ModuleAlreadyBound { module }) if module == identity
        ));
        let renamed = ModuleIdentity {
            package: PackageId("game".into()),
            path: vec!["battle".into()],
        };
        db.bind_module("src/combat.kgr", renamed.clone()).unwrap();
        assert_eq!(old.module(&identity).unwrap().id(), id);
        assert!(db.snapshot().module(&identity).is_none());
        assert!(db.snapshot().file(id).unwrap().revision() > old.file(id).unwrap().revision());
        db.close_overlay("src/combat.kgr").unwrap();
        assert_eq!(db.snapshot().module(&renamed).unwrap().text(), "base");
        let collision = ModuleIdentity::single_file(db.source_name("reserved.kgr").unwrap());
        db.bind_module("other.kgr", collision).unwrap();
        assert!(matches!(
            db.set("reserved.kgr", "base".into(), SourceLayer::Base),
            Err(SourceDatabaseError::SourceModuleAlreadyBound)
        ));
    }

    #[test]
    fn project_paths_and_virtual_uris_have_unambiguous_identity() {
        let mut db = SourceDatabase::new("C:/project").unwrap();
        let file = db
            .set("src/../main.kgr", "base".into(), SourceLayer::Base)
            .unwrap();
        assert_eq!(db.file_id("file:///c:/project/main.kgr"), Some(file));
        assert_eq!(db.file_id("C:\\project\\main.kgr"), Some(file));
        assert_eq!(db.source_name("C:/../../main.kgr").unwrap(), "c:/main.kgr");
        assert_eq!(
            db.source_name("C:relative.kgr"),
            Err(SourceNameError::UnsupportedDriveRelativePath)
        );
        let virtual_file = db
            .set("memory://main.kgr", "virtual".into(), SourceLayer::Base)
            .unwrap();
        assert_eq!(
            db.snapshot().file(virtual_file).unwrap().name(),
            "memory://main.kgr"
        );
        assert_ne!(file, virtual_file);
    }

    #[test]
    fn virtual_uri_schemes_preserve_opaque_identity_without_filesystem_folding() {
        let mut db = SourceDatabase::new("C:/project").unwrap();
        for (input, normalized) in [
            ("UNTITLED:Untitled-1", "untitled:Untitled-1"),
            (
                "Editor+1.-:Case/../Name%2f?Query#Fragment\\Tail",
                "editor+1.-:Case/../Name%2f?Query#Fragment\\Tail",
            ),
            ("MEMORY://Host/../Name%2F", "memory://Host/../Name%2F"),
            ("empty:", "empty:"),
        ] {
            assert_eq!(normalize_source_name(input).unwrap(), normalized);
            assert_eq!(db.source_name(input).unwrap(), normalized);
            assert_eq!(normalize_source_name(normalized).unwrap(), normalized);
            let id = db
                .set(input, "virtual".into(), SourceLayer::Overlay)
                .unwrap();
            assert_eq!(db.file_id(normalized), Some(id));
            assert_eq!(db.snapshot().file(id).unwrap().name(), normalized);
            let old = db.snapshot();
            db.close_overlay(normalized).unwrap();
            assert!(db.snapshot().file(id).is_none());
            assert_eq!(old.file(id).unwrap().text(), "virtual");
            assert_eq!(
                db.set(normalized, "reopened".into(), SourceLayer::Overlay)
                    .unwrap(),
                id
            );
        }
        let id = db.file_id("untitled:Untitled-1").unwrap();
        for other in [
            "untitled:untitled-1",
            "untitled:%55ntitled-1",
            "./untitled:Untitled-1",
            "Untitled-1",
        ] {
            let other_id = db.set(other, "distinct".into(), SourceLayer::Base).unwrap();
            assert_ne!(other_id, id, "{other}");
        }
        assert_eq!(
            db.source_name("./untitled:Untitled-1").unwrap(),
            "c:/project/untitled:Untitled-1"
        );
    }

    #[test]
    fn scheme_grammar_reserves_single_letter_prefixes_for_windows_paths() {
        let db = SourceDatabase::new("C:/project").unwrap();
        for path in ["c:foo", "C:", "x:opaque", "X:?query"] {
            assert_eq!(
                db.source_name(path),
                Err(SourceNameError::UnsupportedDriveRelativePath),
                "{path}"
            );
        }
        for path in ["C:/foo", "C:\\foo", "C://foo"] {
            assert_eq!(db.source_name(path).unwrap(), "c:/foo");
        }
        for path in ["1scheme:Name", "+scheme:Name", "scheme_name:Name"] {
            assert_eq!(db.source_name(path).unwrap(), format!("c:/project/{path}"));
        }
    }

    #[test]
    fn local_file_uris_decode_paths_but_virtual_uris_cannot_be_roots_or_loaded() {
        let mut db = SourceDatabase::new("FiLe://LOCALHOST/C:/project").unwrap();
        let id = db
            .set("C:/project/a b#%中.kgr", "base".into(), SourceLayer::Base)
            .unwrap();
        for uri in [
            "file:///C:/project/a%20b%23%25%E4%B8%AD.kgr",
            "FILE://localhost/c%3A/project/a%20b%23%25%E4%B8%AD.kgr",
            "file:/C:/project/a%20b%23%25%E4%B8%AD.kgr",
        ] {
            assert_eq!(db.file_id(uri), Some(id), "{uri}");
        }
        for uri in ["untitled:Untitled-1", "memory://Main", "empty:"] {
            assert!(
                matches!(
                    SourceDatabase::new(uri),
                    Err(SourceDatabaseError::RootRequiresAbsolutePath)
                ),
                "{uri}"
            );
            let error = db.load_file(uri).unwrap_err();
            assert!(matches!(
                error,
                SourceDatabaseError::VirtualSourceRequiresHost
            ));
            assert_eq!(
                error.to_string(),
                "virtual sources must be supplied by the host"
            );
        }
        for (uri, expected) in [
            (
                "file:relative.kgr",
                SourceNameError::FileUriRequiresAbsolutePath,
            ),
            ("file:///bad%", SourceNameError::InvalidUriEscape),
            ("file:///bad%GG", SourceNameError::InvalidUriEscape),
            ("file:///bad%00", SourceNameError::ContainsNul),
            ("file:///bad%FF", SourceNameError::InvalidUriUtf8),
        ] {
            assert_eq!(db.source_name(uri), Err(expected), "{uri}");
        }
        let before = db.snapshot();
        let directory = normalize_source_name(env!("CARGO_MANIFEST_DIR")).unwrap();
        let error = db.load_file(&format!("{directory}/.")).unwrap_err();
        let SourceDatabaseError::ReadFile { path, source } = &error else {
            panic!("expected file read error, got {error:?}");
        };
        assert_eq!(path, &directory);
        assert!(error.source().is_some_and(|cause| cause.is::<io::Error>()));
        assert_eq!(error.to_string(), format!("{directory}: {source}"));
        assert_eq!(db.snapshot().revision(), before.revision());
        assert_eq!(db.snapshot().file(id).unwrap().text(), "base");
    }

    #[test]
    fn unc_paths_and_nonlocal_file_authorities_are_rejected_without_mutation() {
        let mut db = SourceDatabase::new("C:/project").unwrap();
        let id = db
            .set("main.kgr", "base".into(), SourceLayer::Base)
            .unwrap();
        let before = db.snapshot();
        let unc = SourceNameError::UnsupportedUncPath;
        let remote = SourceNameError::UnsupportedRemoteFileAuthority;
        for (name, expected) in [
            ("//server/share/main.kgr", unc),
            ("\\\\server\\share\\main.kgr", unc),
            ("/\\server/share/main.kgr", unc),
            ("file://server/share/main.kgr", remote),
            ("FILE://server/C:/project/main.kgr", remote),
            ("file://localhost:80/share/main.kgr", remote),
            ("file:////server/share/main.kgr", unc),
            ("file://localhost//server/share/main.kgr", unc),
            ("file:///%2Fserver/share/main.kgr", unc),
            ("file:///%5Cserver/share/main.kgr", unc),
        ] {
            assert_eq!(normalize_source_name(name), Err(expected), "{name}");
            assert!(
                matches!(
                    SourceDatabase::new(name),
                    Err(SourceDatabaseError::Name(error)) if error == expected
                ),
                "{name}"
            );
            assert!(
                matches!(
                    db.set(name, "invalid".into(), SourceLayer::Overlay),
                    Err(SourceDatabaseError::Name(error)) if error == expected
                ),
                "{name}"
            );
            let error = db.load_file(name).unwrap_err();
            assert!(
                matches!(error, SourceDatabaseError::Name(error) if error == expected),
                "{name}"
            );
            assert_eq!(error.to_string(), expected.to_string());
        }
        assert_eq!(db.snapshot().revision(), before.revision());
        assert_eq!(db.snapshot().files().count(), 1);
        assert_eq!(db.file_id("main.kgr"), Some(id));
        assert_eq!(db.snapshot().file(id).unwrap().text(), "base");
    }

    #[test]
    fn overlays_and_snapshots_preserve_identity_and_revisions() {
        let mut db = SourceDatabase::default();
        let id = db
            .set("C:\\project\\a b.kgr", "disk".into(), SourceLayer::Base)
            .unwrap();
        let old = db.snapshot();
        let span = old.file(id).unwrap().span(Span::new(0, 4)).unwrap();
        assert_eq!(
            db.set(
                "file:///C:/project/a%20b.kgr",
                "editor".into(),
                SourceLayer::Overlay
            )
            .unwrap(),
            id
        );
        db.set("c:/project/a b.kgr", "changed".into(), SourceLayer::Base)
            .unwrap();
        assert_eq!(old.file(id).unwrap().text(), "disk");
        assert_eq!(db.snapshot().file(id).unwrap().text(), "editor");
        assert!(!db.snapshot().contains(span));
        db.close_overlay("c:/project/a b.kgr").unwrap();
        assert_eq!(db.snapshot().file(id).unwrap().text(), "changed");
    }

    #[test]
    fn unicode_positions_reject_partial_codepoints_and_crlf() {
        let source = SourceFile::new("unicode", "中😀x\r\n后\n");
        for encoding in [PositionEncoding::Utf8, PositionEncoding::Utf16] {
            for offset in [0, 3, 7, 8, 10, 13, 14] {
                let position = source.position(offset, encoding).unwrap();
                assert_eq!(source.offset(position, encoding), Some(offset));
            }
            assert!(source.position(9, encoding).is_none());
            assert!(source.position(2, encoding).is_none());
        }
        assert!(
            source
                .offset(
                    Position {
                        line: 0,
                        character: 2
                    },
                    PositionEncoding::Utf16
                )
                .is_none()
        );
    }
}

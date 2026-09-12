use std::{collections::BTreeMap, sync::Arc};

use crate::{
    SourceFile,
    identity::{FileId, Revision},
};

#[derive(Debug, Clone, Copy)]
pub enum SourceLayer {
    Base,
    Overlay,
}

#[derive(Debug, Clone)]
struct Document {
    id: FileId,
    base: Option<String>,
    overlay: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct SourceSnapshot {
    revision: Revision,
    files: Arc<BTreeMap<FileId, Arc<SourceFile>>>,
}

impl SourceSnapshot {
    pub fn revision(&self) -> Revision {
        self.revision
    }
    pub fn file(&self, file: FileId) -> Option<&Arc<SourceFile>> {
        self.files.get(&file)
    }
    pub fn files(&self) -> impl Iterator<Item = &Arc<SourceFile>> {
        self.files.values()
    }
    pub fn contains(&self, span: crate::identity::FileSpan) -> bool {
        self.file(span.file)
            .is_some_and(|file| file.revision() == span.revision && file.span(span.range).is_some())
    }
}

#[derive(Debug)]
pub struct SourceDatabase {
    root: String,
    documents: BTreeMap<String, Document>,
    snapshot: SourceSnapshot,
}

impl Default for SourceDatabase {
    fn default() -> Self {
        Self::new(
            std::env::current_dir()
                .expect("working directory unavailable")
                .to_string_lossy()
                .as_ref(),
        )
        .expect("working directory is not a valid source root")
    }
}

impl SourceDatabase {
    /// Capture an absolute project root once; later working-directory changes
    /// cannot change the meaning of relative source names in this database.
    pub fn new(root: &str) -> Result<Self, String> {
        let root = normalize_source_name(root)?;
        if !is_absolute_path(&root) || root.contains("://") {
            return Err("source root must be an absolute filesystem path".into());
        }
        Ok(Self {
            root,
            documents: BTreeMap::new(),
            snapshot: SourceSnapshot::default(),
        })
    }

    pub fn source_name(&self, name: &str) -> Result<String, String> {
        let name = normalize_source_name(name)?;
        if is_absolute_path(&name) || name.contains("://") {
            Ok(name)
        } else {
            normalize_source_name(&format!("{}/{name}", self.root))
        }
    }

    pub fn load_file(&mut self, path: &str) -> Result<FileId, String> {
        let name = self.source_name(path)?;
        if name.contains("://") {
            return Err("virtual sources must be supplied by the host".into());
        }
        let text = std::fs::read_to_string(&name).map_err(|error| format!("{name}: {error}"))?;
        self.set(&name, text, SourceLayer::Base)
    }

    pub fn snapshot(&self) -> SourceSnapshot {
        self.snapshot.clone()
    }

    pub fn set(&mut self, name: &str, text: String, layer: SourceLayer) -> Result<FileId, String> {
        let name = self.source_name(name)?;
        let document = self
            .documents
            .entry(name.clone())
            .or_insert_with(|| Document {
                id: FileId::fresh(),
                base: None,
                overlay: None,
            });
        match layer {
            SourceLayer::Base => document.base = Some(text),
            SourceLayer::Overlay => document.overlay = Some(text),
        }
        let id = document.id;
        self.refresh(&name);
        Ok(id)
    }

    pub fn close_overlay(&mut self, name: &str) -> Result<(), String> {
        let name = self.source_name(name)?;
        if let Some(document) = self.documents.get_mut(&name) {
            document.overlay = None;
        }
        self.refresh(&name);
        Ok(())
    }

    pub fn file_id(&self, name: &str) -> Option<FileId> {
        self.documents
            .get(&self.source_name(name).ok()?)
            .map(|doc| doc.id)
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
            .map(|file| file.text())
            == text.map(String::as_str)
        {
            return;
        }
        self.snapshot.revision.0 = self
            .snapshot
            .revision
            .0
            .checked_add(1)
            .expect("source revision exhausted");
        let files = Arc::make_mut(&mut self.snapshot.files);
        if let Some(text) = text {
            files.insert(
                document.id,
                Arc::new(
                    SourceFile::new(name, text).with_identity(document.id, self.snapshot.revision),
                ),
            );
        } else {
            files.remove(&document.id);
        }
    }
}

/// Lexical normalization only: virtual/unsaved sources need not exist on disk.
pub fn normalize_source_name(name: &str) -> Result<String, String> {
    if name.contains('\0') {
        return Err("source name contains NUL".into());
    }
    if let Some((scheme, rest)) = name.split_once("://")
        && scheme != "file"
    {
        if scheme.is_empty()
            || rest.is_empty()
            || !scheme
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.'))
        {
            return Err("invalid virtual source URI".into());
        }
        return Ok(format!("{}://{rest}", scheme.to_ascii_lowercase()));
    }
    let mut path = if let Some(uri) = name.strip_prefix("file://") {
        let uri = uri
            .strip_prefix("localhost/")
            .map(|p| format!("/{p}"))
            .unwrap_or_else(|| uri.to_owned());
        if !uri.starts_with('/') {
            return Err("remote file URI authorities are unsupported".into());
        }
        let mut bytes = Vec::new();
        let input = uri.as_bytes();
        let mut cursor = 0;
        while cursor < input.len() {
            if input[cursor] == b'%' {
                let digits = input
                    .get(cursor + 1..cursor + 3)
                    .ok_or("invalid URI escape")?;
                let digits = std::str::from_utf8(digits).map_err(|_| "invalid URI escape")?;
                bytes.push(u8::from_str_radix(digits, 16).map_err(|_| "invalid URI escape")?);
                cursor += 3;
            } else {
                bytes.push(input[cursor]);
                cursor += 1;
            }
        }
        String::from_utf8(bytes).map_err(|_| "source URI is not UTF-8")?
    } else {
        name.to_owned()
    }
    .replace('\\', "/");
    if path.contains('\0') {
        return Err("source name contains NUL".into());
    }
    if path.starts_with('/') && path.as_bytes().get(2) == Some(&b':') {
        path.remove(0);
    }
    let drive = path.as_bytes().get(1) == Some(&b':');
    if drive {
        if !path.as_bytes()[0].is_ascii_alphabetic() || path.as_bytes().get(2) != Some(&b'/') {
            return Err("drive-relative source paths are unsupported".into());
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
        return Err("empty source name".into());
    }
    Ok(normalized)
}

fn is_absolute_path(path: &str) -> bool {
    path.starts_with('/')
        || (path.as_bytes().get(1) == Some(&b':') && path.as_bytes().get(2) == Some(&b'/'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Span,
        line_index::{Position, PositionEncoding},
    };

    #[test]
    fn project_paths_and_virtual_uris_have_unambiguous_identity() {
        let mut db = SourceDatabase::new("C:/project").unwrap();
        let file = db
            .set("src/../main.kgr", "base".into(), SourceLayer::Base)
            .unwrap();
        assert_eq!(db.file_id("file:///c:/project/main.kgr"), Some(file));
        assert_eq!(db.file_id("C:\\project\\main.kgr"), Some(file));
        assert_eq!(db.source_name("C:/../../main.kgr").unwrap(), "c:/main.kgr");
        assert!(db.source_name("C:relative.kgr").is_err());
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

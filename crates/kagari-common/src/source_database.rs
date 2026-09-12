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

#[derive(Debug, Default)]
pub struct SourceDatabase {
    documents: BTreeMap<String, Document>,
    snapshot: SourceSnapshot,
}

impl SourceDatabase {
    pub fn snapshot(&self) -> SourceSnapshot {
        self.snapshot.clone()
    }

    pub fn set(&mut self, name: &str, text: String, layer: SourceLayer) -> Result<FileId, String> {
        let name = normalize_source_name(name)?;
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
        let name = normalize_source_name(name)?;
        if let Some(document) = self.documents.get_mut(&name) {
            document.overlay = None;
        }
        self.refresh(&name);
        Ok(())
    }

    pub fn file_id(&self, name: &str) -> Option<FileId> {
        self.documents
            .get(&normalize_source_name(name).ok()?)
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
    if path.as_bytes().get(1) == Some(&b':') {
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
                } else if !absolute {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Span,
        line_index::{Position, PositionEncoding},
    };

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

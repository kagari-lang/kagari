//! Declared module-tree discovery shared by virtual and filesystem sources.

use crate::{error::PackageError, path};
use kagari_common::{cancellation::CancellationToken, identity::MAX_IDENTITY_PATH_SEGMENTS};
use kagari_source::{diagnostic::Diagnostic, source::SourceFile};
use kagari_syntax::{
    ast::{
        item::{Item, ModuleDef},
        traits::AstNode,
    },
    parser::parse_with_cancellation,
};
use std::{collections::BTreeMap, ops::Range, sync::Arc};

/// One immutable source acquired from an approved input root.
#[derive(Debug, Clone)]
pub struct SourceText {
    /// Canonical provider key, used to reject physical aliases within a module tree.
    pub key: String,
    /// Display origin used by diagnostics; not a portable package identity.
    pub origin: String,
    pub text: Arc<str>,
}

/// Reads relative source paths within a caller-approved input root.
///
/// Implementations must enforce their path/symlink policy and the byte limit before
/// allocating the complete file. Absence is `None`; access and decoding errors must
/// not be reported as absence. Reading has no script-visible execution side effects.
pub trait SourceInput {
    fn read(
        &self,
        path: &str,
        max_bytes: usize,
        cancel: &CancellationToken,
    ) -> Result<Option<SourceText>, PackageError>;
}

/// Host-owned source collection with canonical, relative, slash-separated keys.
#[derive(Debug, Clone, Default)]
pub struct VirtualSources {
    pub(crate) files: BTreeMap<String, Arc<str>>,
}

impl VirtualSources {
    /// Captures the supplied files, rejecting duplicate normalized paths.
    pub fn new(
        files: impl IntoIterator<Item = (String, Arc<str>)>,
        cancel: &CancellationToken,
    ) -> Result<Self, PackageError> {
        let mut result = BTreeMap::new();
        for (name, text) in files {
            cancel.check()?;
            let key = path::join("", &name, &name)?;
            if key.is_empty() || result.insert(key, text).is_some() {
                return Err(PackageError::input(
                    &name,
                    "empty or duplicate normalized source path",
                ));
            }
        }
        cancel.check()?;
        Ok(Self { files: result })
    }
}

impl SourceInput for VirtualSources {
    fn read(
        &self,
        path: &str,
        max_bytes: usize,
        cancel: &CancellationToken,
    ) -> Result<Option<SourceText>, PackageError> {
        cancel.check()?;
        let key = path::join("", path, path)?;
        let Some(text) = self.files.get(&key) else {
            return Ok(None);
        };
        if text.len() > max_bytes {
            return Err(PackageError::input(path, "source exceeds byte limit"));
        }
        Ok(Some(SourceText {
            origin: key.clone(),
            key,
            text: text.clone(),
        }))
    }
}

/// Bounds for one package's declared module tree, including inline modules.
#[derive(Debug, Clone, Copy)]
pub struct ModuleLimits {
    pub max_modules: usize,
    pub max_file_bytes: usize,
    pub max_total_bytes: usize,
}

impl Default for ModuleLimits {
    fn default() -> Self {
        Self {
            max_modules: 4096,
            max_file_bytes: 4 * 1024 * 1024,
            max_total_bytes: 64 * 1024 * 1024,
        }
    }
}

/// A module's source location. Inline ranges cover the complete `mod name { ... }`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleSource {
    /// Key into [`ModuleTree::files`]; a portable input-root-relative source path.
    pub file: String,
    pub inline_range: Option<Range<usize>>,
}

/// A complete declared module tree, with the unnamed root at `Vec::new()`.
///
/// `mod foo;` loads `foo.kgr` or `foo/mod.kgr` relative to the module directory.
/// Both present is an error. `mod foo { mod bar; }` searches `foo/bar.kgr` and
/// `foo/bar/mod.kgr`. Inline nodes reuse their containing file's immutable text.
#[derive(Debug, Clone)]
pub struct ModuleTree {
    pub modules: BTreeMap<Vec<String>, ModuleSource>,
    pub files: BTreeMap<String, SourceText>,
}

/// Recoverable source discovery; incomplete trees cannot become compilation inputs.
#[derive(Debug, Clone)]
pub struct ModuleDiscovery {
    tree: ModuleTree,
    /// Syntax diagnostics paired with the exact captured source that owns their ranges.
    diagnostics: Vec<(SourceFile, Vec<Diagnostic>)>,
    /// Unavailable, ambiguous or otherwise invalid module inputs.
    errors: Vec<PackageError>,
}

impl ModuleDiscovery {
    /// Captured files, including syntax-invalid text. No completeness is implied.
    pub fn files(&self) -> &BTreeMap<String, SourceText> {
        &self.tree.files
    }

    /// Discovered module locations; missing children may remain undiscovered.
    pub fn modules(&self) -> &BTreeMap<Vec<String>, ModuleSource> {
        &self.tree.modules
    }

    pub fn diagnostics(&self) -> &[(SourceFile, Vec<Diagnostic>)] {
        &self.diagnostics
    }

    pub fn errors(&self) -> &[PackageError] {
        &self.errors
    }

    pub fn is_complete(&self) -> bool {
        self.errors.is_empty() && self.diagnostics.is_empty()
    }

    /// Converts only complete, syntax-valid discovery to a compilation tree.
    pub fn into_complete(self) -> Result<ModuleTree, PackageError> {
        if let Some(error) = self.errors.into_iter().next() {
            return Err(error);
        }
        if let Some((source, diagnostics)) = self.diagnostics.first() {
            let diagnostic = &diagnostics[0];
            return Err(PackageError::Input {
                origin: source.name().into(),
                range: diagnostic.span.map(|span| span.start..span.end),
                message: diagnostic.to_string(),
            });
        }
        Ok(self.tree)
    }

    /// Reads declared modules while retaining independent siblings after local failures.
    /// Syntax-invalid files retain their text but do not authorize child discovery.
    /// Cancellation is returned immediately; no partial result is published for it.
    pub fn load(
        input: &impl SourceInput,
        root: &str,
        limits: ModuleLimits,
        cancel: &CancellationToken,
    ) -> Result<Self, PackageError> {
        cancel.check()?;
        let root = path::join("", root, root)?;
        let mut loader = Loader {
            input,
            limits,
            cancel,
            discovery: Self {
                tree: ModuleTree {
                    modules: BTreeMap::new(),
                    files: BTreeMap::new(),
                },
                diagnostics: Vec::new(),
                errors: Vec::new(),
            },
            physical: BTreeMap::new(),
            total_bytes: 0,
        };
        let result = loader.read(&root).and_then(|source| {
            let source = source.ok_or_else(|| PackageError::input(&root, "missing root module"))?;
            loader.file(Vec::new(), &root, path::parent(&root), source)
        });
        loader.retain(result)?;
        cancel.check()?;
        Ok(loader.discovery)
    }
}

impl ModuleTree {
    /// Loads a complete tree, rejecting every discovery failure before compilation.
    pub fn load(
        input: &impl SourceInput,
        root: &str,
        limits: ModuleLimits,
        cancel: &CancellationToken,
    ) -> Result<Self, PackageError> {
        ModuleDiscovery::load(input, root, limits, cancel)?.into_complete()
    }
}

struct Loader<'a, T> {
    input: &'a T,
    limits: ModuleLimits,
    cancel: &'a CancellationToken,
    discovery: ModuleDiscovery,
    physical: BTreeMap<String, String>,
    total_bytes: usize,
}

impl<T: SourceInput> Loader<'_, T> {
    fn read(&self, file: &str) -> Result<Option<SourceText>, PackageError> {
        self.cancel.check()?;
        self.input.read(
            file,
            self.limits
                .max_file_bytes
                .min(self.limits.max_total_bytes.saturating_sub(self.total_bytes)),
            self.cancel,
        )
    }

    fn register(&mut self, module: Vec<String>, source: ModuleSource) -> Result<(), PackageError> {
        self.cancel.check()?;
        if module.len() > MAX_IDENTITY_PATH_SEGMENTS
            || self.discovery.tree.modules.len() >= self.limits.max_modules
        {
            return Err(PackageError::input(
                &source.file,
                "module tree exceeds limit",
            ));
        }
        if self.discovery.tree.modules.contains_key(&module) {
            return Err(PackageError::input(
                &source.file,
                format!("duplicate module {}", module.join("::")),
            ));
        }
        self.discovery.tree.modules.insert(module, source);
        Ok(())
    }

    fn file(
        &mut self,
        module: Vec<String>,
        file: &str,
        directory: &str,
        source: SourceText,
    ) -> Result<(), PackageError> {
        self.register(
            module.clone(),
            ModuleSource {
                file: file.into(),
                inline_range: None,
            },
        )?;
        if let Some(previous) = self.physical.insert(source.key.clone(), file.into()) {
            return Err(PackageError::input(
                &source.origin,
                format!("source already assigned to module at {previous}"),
            ));
        }
        // Enforce the bounds even when a host provider violates its read contract.
        if source.text.len() > self.limits.max_file_bytes
            || source.text.len() > self.limits.max_total_bytes.saturating_sub(self.total_bytes)
        {
            return Err(PackageError::input(
                &source.origin,
                "source exceeds byte limit",
            ));
        }
        self.total_bytes += source.text.len();
        let captured = SourceFile::new(&source.origin, source.text.as_ref());
        self.discovery.tree.files.insert(file.into(), source);
        let parsed = parse_with_cancellation(&captured, self.cancel)?;
        if !parsed.diagnostics().is_empty() {
            self.discovery
                .diagnostics
                .push((captured, parsed.diagnostics().to_vec()));
            return Ok(());
        }
        self.items(parsed.syntax().items(), &module, file, directory)
    }

    fn items(
        &mut self,
        items: impl Iterator<Item = Item>,
        parent: &[String],
        file: &str,
        directory: &str,
    ) -> Result<(), PackageError> {
        for item in items {
            self.cancel.check()?;
            let Item::ModuleDef(declaration) = item else {
                continue;
            };
            let result = self.module(declaration, parent, file, directory);
            self.retain(result)?;
        }
        Ok(())
    }

    fn retain(&mut self, result: Result<(), PackageError>) -> Result<(), PackageError> {
        match result {
            Err(PackageError::Cancelled) => Err(PackageError::Cancelled),
            Err(error) => {
                self.discovery.errors.push(error);
                Ok(())
            }
            Ok(()) => Ok(()),
        }
    }

    fn module(
        &mut self,
        declaration: ModuleDef,
        parent: &[String],
        file: &str,
        directory: &str,
    ) -> Result<(), PackageError> {
        let name = declaration
            .name_text()
            .ok_or_else(|| PackageError::input(file, "module name missing"))?;
        let mut module = parent.to_vec();
        module.push(name.clone());
        let child_directory = path::join(directory, &name, file)?;
        if let Some(block) = declaration.block() {
            let range = declaration.syntax().text_range();
            self.register(
                module.clone(),
                ModuleSource {
                    file: file.into(),
                    inline_range: Some(usize::from(range.start())..usize::from(range.end())),
                },
            )?;
            self.items(block.items(), &module, file, &child_directory)?;
        } else {
            let flat = format!("{child_directory}.kgr");
            let nested = format!("{child_directory}/mod.kgr");
            match (self.read(&flat)?, self.read(&nested)?) {
                (Some(source), None) => self.file(module, &flat, &child_directory, source)?,
                (None, Some(source)) => self.file(module, &nested, &child_directory, source)?,
                (Some(_), Some(_)) => {
                    return Err(PackageError::input(
                        file,
                        format!("ambiguous module {name}: both {flat} and {nested} exist"),
                    ));
                }
                (None, None) => {
                    return Err(PackageError::input(
                        file,
                        format!("missing module {name}: expected {flat} or {nested}"),
                    ));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod input_tests;

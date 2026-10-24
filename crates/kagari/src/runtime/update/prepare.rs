//! Resolve against one selected publication, then check one complete detached candidate.

use crate::{
    CompileResult,
    engine::source::{
        CheckedModule,
        inputs::{
            CheckedInputs, CompilationInputs, SourceEdit, SourceTextBudget, append_source_text,
            copy_source_text, replaced_source_len, source_text_buffer,
        },
    },
    error::{EmbeddingError, RuntimeFailureKind},
    update::{Edit, FunctionUpdateTarget, Update},
};
use kagari_bytecode::{
    artifact::KAGARI_COMPILER_FINGERPRINT, source_package::VerifiedSourcePackage,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionPath, ModuleIdentity, table::DefinitionId},
    span::Span,
};
use kagari_contract::{
    exports::path::PublicPathError,
    source::{AuthoredDeclaration, AuthoredKind, MAX_SOURCE_BYTES},
};
use kagari_hir::{source_inspection::collect_authored_sources, update_source::SourceBodyKind};
use kagari_runtime::{
    Runtime,
    module::{ExecutableModule, Program},
};
use kagari_source::{identity::FileId, source::SourceFile};
use kagari_syntax::{lexer::lex_with_cancellation, token::TokenKind};
use std::{
    collections::{BTreeMap, HashSet},
    sync::Arc,
};

const MAX_EDITS: usize = 1024;

pub(super) fn validate_batch(update: &Update) -> CompileResult<()> {
    update
        .cancel
        .check()
        .map_err(|_| EmbeddingError::Cancelled)?;
    if update.edits.is_empty() || update.edits.len() > MAX_EDITS {
        return Err(source_error("source update requires 1..=1024 edits"));
    }
    let mut bytes = 0usize;
    for edit in &update.edits {
        update
            .cancel
            .check()
            .map_err(|_| EmbeddingError::Cancelled)?;
        bytes = bytes
            .checked_add(edit.text().len())
            .filter(|bytes| *bytes <= MAX_SOURCE_BYTES)
            .ok_or_else(|| source_error("source update exceeds 64 MiB of replacement UTF-8"))?;
    }
    Ok(())
}

struct FunctionEdit<'a> {
    declaration: DefinitionPath,
    range: Span,
    text: &'a str,
}

/// Expected extents are derived only after resolving a checked authored declaration.
/// They validate candidate syntax; callers cannot submit them as edit authority.
struct ExpectedFunction {
    declaration: DefinitionPath,
    physical_module: ModuleIdentity,
    inserted: Span,
}

#[derive(Default)]
struct DocumentEdits<'a> {
    whole: Option<&'a str>,
    functions: Vec<FunctionEdit<'a>>,
}

struct AddedDocument<'a> {
    module: &'a ModuleIdentity,
    name: String,
    text: &'a str,
}

pub(super) fn check_candidate(
    runtime: &Runtime,
    program: &Program,
    current: &ExecutableModule,
    inputs: &CompilationInputs,
    update: &Update,
) -> CompileResult<CheckedModule> {
    let captured = inputs.checked();
    if captured.compiler_contract() != KAGARI_COMPILER_FINGERPRINT
        || captured.root()
            != &current.verified_program().modules()[current.verified_program().root().index()]
                .identity
    {
        return Err(source_error(
            "MissingCompilationContext: capsule compiler/root does not match publication",
        ));
    }
    let snapshot = current.source_snapshot();
    let package = snapshot
        .source_package()
        .ok_or_else(|| source_error("MissingCompilationContext: exact source package is absent"))?;
    let mut documents = BTreeMap::<FileId, DocumentEdits<'_>>::new();
    let mut additions = Vec::new();
    let mut names = HashSet::new();
    let mut declarations = HashSet::new();
    for edit in &update.edits {
        update
            .cancel
            .check()
            .map_err(|_| EmbeddingError::Cancelled)?;
        match edit {
            Edit::Function { target, definition } => {
                let record = match target {
                    FunctionUpdateTarget::Declaration(declaration) => runtime
                        .resolve_source_declaration(program, declaration)
                        .map_err(|error| source_error(error.to_string()))?,
                    FunctionUpdateTarget::Path(path) => public_function(current, package, path)?,
                };
                if !matches!(
                    record.kind,
                    AuthoredKind::Function | AuthoredKind::AsyncFunction
                ) {
                    return Err(source_error(
                        "source update target is not a script function definition",
                    ));
                }
                if !declarations.insert(record.declaration) {
                    return Err(source_error(
                        "duplicate function update target (including public aliases)",
                    ));
                }
                let declaration = package
                    .definitions()
                    .resolve(record.declaration)
                    .map_err(|error| source_error(error.to_string()))?
                    .to_path();
                let file = authored_document(captured, package, &record, &declaration.module)?;
                let edits = documents.entry(file.id()).or_default();
                if edits.whole.is_some() {
                    return Err(source_error("whole-file and function replacements overlap"));
                }
                edits.functions.push(FunctionEdit {
                    declaration,
                    range: record.range,
                    text: definition,
                });
            }
            Edit::File { name, text } => {
                let name = captured.document_name(name, &update.cancel)?;
                if !names.insert(name.clone()) {
                    return Err(source_error("duplicate normalized document update target"));
                }
                let file = named_document(captured, &name, &update.cancel)?;
                let edits = documents.entry(file.id()).or_default();
                if edits.whole.is_some() || !edits.functions.is_empty() {
                    return Err(source_error("whole-file and function replacements overlap"));
                }
                edits.whole = Some(text);
            }
            Edit::Add { module, name, text } => {
                let name = captured.document_name(name, &update.cancel)?;
                if !names.insert(name.clone()) {
                    return Err(source_error("duplicate normalized document update target"));
                }
                for file in captured.sources().files() {
                    if captured.document_name(file.name(), &update.cancel)? == name {
                        return Err(source_error(
                            "added document already exists in the captured inputs",
                        ));
                    }
                }
                additions.push(AddedDocument { module, name, text });
            }
        }
    }
    // Resolve the entire batch and bound its final physical bytes before constructing
    // any replacement strings. Unchanged text and newly added files count too.
    let lengths = preflight_documents(captured, &mut documents, &additions, &update.cancel)?;
    let mut expected = Vec::new();
    let mut edits = Vec::new();
    for addition in additions {
        edits.push(SourceEdit::Add {
            module: addition.module.clone(),
            name: addition.name,
            text: copy_source_text(addition.text, &update.cancel)?,
        });
    }
    for (file, changes) in documents {
        let source = captured
            .sources()
            .file(file)
            .expect("resolved captured document");
        if let Some(text) = changes.whole {
            edits.push(SourceEdit::Replace {
                name: copy_source_text(source.name(), &update.cancel)?,
                text: copy_source_text(text, &update.cancel)?,
            });
        } else {
            let text = replace_functions(
                source,
                changes.functions,
                lengths[&file],
                &mut expected,
                &update.cancel,
            )?;
            edits.push(SourceEdit::Document { file, text });
        }
    }
    let candidate = inputs.with_edits(&edits, &update.cancel)?;
    let checked = candidate.checked().recheck(&update.cancel)?;
    validate_functions(&checked, &expected, &update.cancel)?;
    Ok(checked)
}

fn named_document<'a>(
    captured: &'a CheckedInputs,
    name: &str,
    cancel: &CancellationToken,
) -> CompileResult<&'a Arc<SourceFile>> {
    let mut found = None;
    for file in captured.sources().files() {
        if captured.document_name(file.name(), cancel)? == name {
            if found.is_some() {
                return Err(source_error("ambiguous captured document name"));
            }
            found = Some(file);
        }
    }
    found.ok_or_else(|| source_error(format!("unknown captured document: {name}")))
}

fn authored_document<'a>(
    captured: &'a CheckedInputs,
    package: &VerifiedSourcePackage,
    record: &AuthoredDeclaration,
    module: &ModuleIdentity,
) -> CompileResult<&'a Arc<SourceFile>> {
    let document = &package.records().documents[record.document as usize];
    let mut found = None;
    for file in captured.sources().files() {
        let physical = file.module_identity();
        if physical.package == module.package
            && module.path.starts_with(&physical.path)
            && file.name() == document.uri
            && file.text() == document.text
        {
            if found.is_some() {
                return Err(source_error("ambiguous physical function document"));
            }
            found = Some(file);
        }
    }
    found.ok_or_else(|| {
        source_error("source package definition is not in the matching editable capsule")
    })
}

fn public_function(
    current: &ExecutableModule,
    package: &VerifiedSourcePackage,
    path: &str,
) -> CompileResult<AuthoredDeclaration> {
    let declaration: DefinitionId =
        current
            .exported_function_declaration(path)
            .map_err(|error| match error {
                PublicPathError::InvalidGraph => EmbeddingError::runtime(
                    RuntimeFailureKind::EngineInvariant,
                    "invalid verified public function graph during source update",
                ),
                error => source_error(format!(
                    "invalid public function update target {path:?}: {error:?}"
                )),
            })?;
    package
        .records()
        .declarations
        .iter()
        .find(|record| record.declaration == declaration)
        .cloned()
        .ok_or_else(|| source_error("public function has no exact authored definition"))
}

fn preflight_documents(
    captured: &CheckedInputs,
    documents: &mut BTreeMap<FileId, DocumentEdits<'_>>,
    additions: &[AddedDocument<'_>],
    cancel: &CancellationToken,
) -> CompileResult<BTreeMap<FileId, usize>> {
    let mut budget = SourceTextBudget::default();
    let mut lengths = BTreeMap::new();
    for source in captured.sources().files() {
        cancel.check().map_err(|_| EmbeddingError::Cancelled)?;
        let bytes = match documents.get_mut(&source.id()) {
            Some(changes) => {
                let bytes = match changes.whole {
                    Some(text) => text.len(),
                    None => {
                        function_replacement_len(source.text(), &mut changes.functions, cancel)?
                    }
                };
                lengths.insert(source.id(), bytes);
                bytes
            }
            None => source.text().len(),
        };
        budget.add_document(bytes)?;
    }
    for addition in additions {
        cancel.check().map_err(|_| EmbeddingError::Cancelled)?;
        budget.add_document(addition.text.len())?;
    }
    Ok(lengths)
}

fn function_replacement_len(
    source: &str,
    edits: &mut [FunctionEdit<'_>],
    cancel: &CancellationToken,
) -> CompileResult<usize> {
    cancel.check().map_err(|_| EmbeddingError::Cancelled)?;
    edits.sort_by_key(|edit| (edit.range.start, edit.range.end));
    let mut cursor = 0;
    let mut removed = 0usize;
    let mut inserted = 0usize;
    for edit in edits {
        cancel.check().map_err(|_| EmbeddingError::Cancelled)?;
        if edit.range.start < cursor {
            return Err(source_error(
                "function replacements overlap (including enclosing/named-local definitions)",
            ));
        }
        let original = source
            .get(edit.range.start..edit.range.end)
            .ok_or_else(|| source_error("source package extent does not match captured UTF-8"))?;
        removed = removed
            .checked_add(original.len())
            .ok_or_else(|| source_error("source extent size overflow"))?;
        inserted = inserted
            .checked_add(edit.text.len())
            .ok_or_else(|| source_error("source replacement size overflow"))?;
        cursor = edit.range.end;
    }
    replaced_source_len(source.len(), removed, inserted)
}

fn replace_functions(
    source: &SourceFile,
    edits: Vec<FunctionEdit<'_>>,
    bytes: usize,
    expected: &mut Vec<ExpectedFunction>,
    cancel: &CancellationToken,
) -> CompileResult<String> {
    let mut cursor = 0;
    let mut text = source_text_buffer(bytes, cancel)?;
    for edit in edits {
        append_source_text(&mut text, &source.text()[cursor..edit.range.start], cancel)?;
        let start = text.len();
        append_source_text(&mut text, edit.text, cancel)?;
        expected.push(ExpectedFunction {
            declaration: edit.declaration,
            physical_module: source.module_identity().clone(),
            inserted: Span::new(start, text.len()),
        });
        cursor = edit.range.end;
    }
    append_source_text(&mut text, &source.text()[cursor..], cancel)?;
    debug_assert_eq!(text.len(), bytes);
    Ok(text)
}

fn validate_functions(
    checked: &CheckedModule,
    expected: &[ExpectedFunction],
    cancel: &CancellationToken,
) -> CompileResult<()> {
    if expected.is_empty() {
        return Ok(());
    }
    let index = collect_authored_sources(checked.program(), cancel).map_err(|error| {
        if cancel.check().is_err() {
            EmbeddingError::Cancelled
        } else {
            source_error(error.to_string())
        }
    })?;
    let mut declarations = BTreeMap::new();
    for record in &index.records().declarations {
        let path = index
            .definitions()
            .resolve(record.declaration)
            .map_err(|error| source_error(error.to_string()))?
            .to_path();
        declarations.insert(path, record);
    }
    for expected in expected {
        cancel.check().map_err(|_| EmbeddingError::Cancelled)?;
        let record = declarations.get(&expected.declaration).ok_or_else(|| {
            source_error("replacement must preserve its authored function identity and owner")
        })?;
        if !matches!(
            record.kind,
            Some(SourceBodyKind::Function | SourceBodyKind::AsyncFunction)
        ) {
            return Err(source_error(
                "replacement must be one complete script function definition",
            ));
        }
        let document = &index.records().documents[record.document];
        let physical = checked
            .inputs()
            .sources()
            .file(document.origin_id())
            .ok_or_else(|| source_error("replacement is not in an authored physical document"))?;
        if physical.module_identity() != &expected.physical_module
            || record.range.start < expected.inserted.start
            || record.range.end > expected.inserted.end
        {
            return Err(source_error(
                "replacement escapes its original function owner",
            ));
        }
        let before = physical
            .text()
            .get(expected.inserted.start..record.range.start)
            .ok_or_else(|| source_error("invalid replacement extent"))?;
        let after = physical
            .text()
            .get(record.range.end..expected.inserted.end)
            .ok_or_else(|| source_error("invalid replacement extent"))?;
        if !trivia_only(before, cancel)? || !trivia_only(after, cancel)? {
            return Err(source_error(
                "replacement must contain exactly one complete function definition",
            ));
        }
    }
    Ok(())
}

fn trivia_only(text: &str, cancel: &CancellationToken) -> CompileResult<bool> {
    let tokens = lex_with_cancellation(text, cancel).map_err(|_| EmbeddingError::Cancelled)?;
    Ok(tokens
        .iter()
        .all(|token| token.kind.is_trivia() || token.kind == TokenKind::Eof))
}

fn source_error(message: impl Into<String>) -> EmbeddingError {
    EmbeddingError::Source {
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(range: Span, text: &str) -> FunctionEdit<'_> {
        FunctionEdit {
            declaration: DefinitionPath {
                module: ModuleIdentity::single_file("preflight.kgr"),
                path: Vec::new(),
            },
            range,
            text,
        }
    }

    #[test]
    fn function_size_preflight_validates_utf8_and_overlap_before_building_text() {
        let cancel = CancellationToken::default();
        let source = SourceFile::new("preflight.kgr", "αxxβyy");
        let mut edits = vec![edit(Span::new(6, 8), "Z"), edit(Span::new(2, 4), "🦀")];
        let bytes = function_replacement_len(source.text(), &mut edits, &cancel).unwrap();
        assert_eq!(bytes, "α🦀βZ".len());
        let mut expected = Vec::new();
        let output = replace_functions(&source, edits, bytes, &mut expected, &cancel).unwrap();
        assert_eq!(output, "α🦀βZ");
        assert_eq!(expected[0].inserted, Span::new(2, 6));
        assert_eq!(expected[1].inserted, Span::new(8, 9));
        for mut invalid in [
            vec![edit(Span::new(1, 2), "x")],
            vec![edit(Span::new(0, 4), "x"), edit(Span::new(2, 4), "y")],
            vec![edit(Span::new(0, usize::MAX), "x")],
        ] {
            assert!(function_replacement_len(source.text(), &mut invalid, &cancel).is_err());
        }
        cancel.cancel();
        assert!(matches!(
            function_replacement_len(source.text(), &mut [edit(Span::new(2, 4), "x")], &cancel),
            Err(EmbeddingError::Cancelled)
        ));
    }
}

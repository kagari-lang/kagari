//! Complete checked declaration extents and their retained physical source owners.
//!
//! This is a preparation/tooling index, not an execution lease or edit capability.
//! For `mod m { fn f() {} }`, `f` belongs to `m`, but its range is in the original
//! containing document. Named locals are indexed independently of reachability.

use crate::{
    CheckedAnalysis, hir::item::Item, program::CheckedProgram, resolver::resolved::ResolvedName,
    typeck::FunctionImplementation, update_source::SourceBodyKind,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMappingError, check_cancel},
        metadata::DefinitionMetadata,
        scoped::DefinitionReferences,
        table::DefinitionId,
    },
    span::Span,
};
use kagari_source::source::SourceFile;
use std::{collections::HashSet, sync::Arc};

const MAX_RECORDS: usize = 1_000_000;
const MAX_BYTES: usize = 64 * 1024 * 1024;

/// One complete declaration, including its signature and body when present.
#[derive(Debug, Clone)]
pub struct AuthoredSource {
    pub declaration: DefinitionId,
    /// None denotes a non-body declaration, such as a type or impl block.
    pub kind: Option<SourceBodyKind>,
    pub document: usize,
    pub range: Span,
}

/// Physical documents are deduplicated by retained source identity, not URI alone.
#[derive(Debug, Clone)]
pub struct AuthoredSourceIndex {
    pub documents: Vec<Arc<SourceFile>>,
    pub declarations: Vec<AuthoredSource>,
}

impl DefinitionReferences for AuthoredSourceIndex {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        for record in &self.declarations {
            check_cancel(cancel)?;
            visit(&record.declaration)?;
        }
        Ok(())
    }
}

/// Collect exact full syntax extents from the checked closure, not navigation names.
///
/// Fails on missing identities, invalid physical/UTF-8 ranges, cancellation or
/// the aggregate 64 MiB/one-million-record budget. No disk/editor lookup occurs.
pub fn collect_authored_sources(
    program: &CheckedProgram,
    cancel: &CancellationToken,
) -> Result<DefinitionMetadata<AuthoredSourceIndex>, DefinitionMappingError> {
    let mut collector = Collector {
        index: AuthoredSourceIndex {
            documents: Vec::new(),
            declarations: Vec::new(),
        },
        seen: HashSet::new(),
        bytes: 0,
    };
    for module in program.modules() {
        check_cancel(cancel)?;
        let hir = &module.lowered.module;
        let map = &module.lowered.source_map;
        for function in &hir.functions {
            let typed = module
                .typed
                .functions
                .iter()
                .find(|typed| typed.id == function.id)
                .ok_or(DefinitionMappingError::InvalidContract)?;
            let kind = match typed.implementation {
                FunctionImplementation::Native(_) => SourceBodyKind::Native,
                FunctionImplementation::Required => SourceBodyKind::Required,
                FunctionImplementation::Script if function.is_async => {
                    SourceBodyKind::AsyncFunction
                }
                FunctionImplementation::Script => SourceBodyKind::Function,
            };
            collector.named(
                module,
                ResolvedName::Function(function.id),
                map.function_span(function.id),
                Some(kind),
                cancel,
            )?;
        }
        for item in &hir.consts {
            collector.named(
                module,
                ResolvedName::Const(item.id),
                map.const_span(item.id),
                Some(SourceBodyKind::Constant),
                cancel,
            )?;
        }
        for item in &hir.statics {
            collector.named(
                module,
                ResolvedName::Static(item.id),
                map.static_span(item.id),
                Some(SourceBodyKind::StaticInitializer),
                cancel,
            )?;
        }
        for item in &hir.items {
            let name = match *item {
                Item::Function(_) | Item::Const(_) | Item::Static(_) => continue,
                Item::Struct(id) => ResolvedName::Struct(id),
                Item::Enum(id) => ResolvedName::Enum(id),
                Item::Trait(id) => ResolvedName::Trait(id),
                Item::OpaqueType(id) => ResolvedName::OpaqueType(id),
                Item::TypeAlias(id) => ResolvedName::TypeAlias(id),
                Item::Module(_) => continue,
                Item::Impl(id) => {
                    let declaration = *module
                        .declarations
                        .impl_identity(id)
                        .ok_or(DefinitionMappingError::InvalidContract)?;
                    collector.add(module, declaration, map.item_span(*item), None, cancel)?;
                    continue;
                }
            };
            collector.named(module, name, map.item_span(*item), None, cancel)?;
        }
    }
    DefinitionMetadata::checked(
        program.root().definitions().clone(),
        collector.index,
        cancel,
    )
}

struct Collector {
    index: AuthoredSourceIndex,
    seen: HashSet<DefinitionId>,
    bytes: usize,
}

impl Collector {
    fn named(
        &mut self,
        module: &CheckedAnalysis,
        name: ResolvedName,
        range: Span,
        kind: Option<SourceBodyKind>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        let declaration = *module
            .declarations
            .definition(name)
            .ok_or(DefinitionMappingError::InvalidContract)?;
        self.add(module, declaration, range, kind, cancel)
    }

    fn add(
        &mut self,
        module: &CheckedAnalysis,
        declaration: DefinitionId,
        range: Span,
        kind: Option<SourceBodyKind>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        if !self.seen.insert(declaration) {
            return Err(DefinitionMappingError::InvalidContract);
        }
        if self.seen.len() > MAX_RECORDS {
            return Err(DefinitionMappingError::LimitExceeded);
        }
        let source = &module.lowered.source;
        let physical = source
            .span(range)
            .ok_or(DefinitionMappingError::InvalidContract)?;
        let origin = if physical.file == source.origin_id() {
            source.as_ref()
        } else {
            source
                .copied_sources()
                .find(|source| {
                    source.id() == physical.file && source.revision() == physical.revision
                })
                .ok_or(DefinitionMappingError::InvalidContract)?
        };
        let physical_text = origin
            .text()
            .get(physical.range.start..physical.range.end)
            .filter(|text| !text.is_empty())
            .ok_or(DefinitionMappingError::InvalidContract)?;
        let local_text = source
            .text()
            .get(range.start..range.end)
            .ok_or(DefinitionMappingError::InvalidContract)?;
        if physical_text != local_text {
            return Err(DefinitionMappingError::InvalidContract);
        }
        let document = match self.index.documents.iter().position(|source| {
            source.origin_id() == physical.file && source.revision() == physical.revision
        }) {
            Some(index) => index,
            None => {
                self.bytes = self
                    .bytes
                    .checked_add(origin.text().len())
                    .and_then(|n| n.checked_add(origin.name().len()))
                    .ok_or(DefinitionMappingError::LimitExceeded)?;
                if self.bytes > MAX_BYTES {
                    return Err(DefinitionMappingError::LimitExceeded);
                }
                let index = self.index.documents.len();
                self.index.documents.push(Arc::new(origin.clone()));
                index
            }
        };
        self.index.declarations.push(AuthoredSource {
            declaration,
            kind,
            document,
            range: physical.range,
        });
        Ok(())
    }
}

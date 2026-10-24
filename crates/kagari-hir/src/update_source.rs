//! Authored provenance and static initializer evidence collected before optimization.
//!
//! Only static expressions retain non-trivia tokens and local checked resolutions.
//! Ordinary bodies contribute identities, never their computations or dependencies.

use crate::{
    CheckedAnalysis,
    declarations::DeclarationId,
    hir::ids::{BodyOwner, HirOwner},
    resolver::resolved::ResolvedName,
    typeck::FunctionImplementation,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMappingError, check_cancel},
        table::DefinitionId,
    },
    span::Span,
};
use kagari_syntax::{lexer::lex_with_cancellation, token::TokenKind};
use kagari_types::ty::Ty;
use std::collections::HashMap;

mod facts;
mod mapping;
mod order;
#[cfg(test)]
mod tests;

/// An exact non-trivia lexeme. String contents, escapes and raw delimiters survive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceToken {
    pub kind: TokenKind,
    pub text: String,
}

/// Half-open positions in an initializer's non-trivia tokens, not source offsets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TokenRange {
    pub start: usize,
    pub end: usize,
}

/// Role of a local checked fact; references never request callee-body comparison.
/// Calls retain target IDs and type arguments; signatures retain parameter/result
/// types. Trait/protocol facts retain receiver/interface types when available.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum FactKind {
    Reference,
    Type,
    Call,
    FunctionValue,
    TraitCall,
    Signature,
    Protocol,
    Member,
    Construction,
    DeclaredCoercion,
    BoundCoercion,
    IntrinsicCoercion,
    InterfaceUpcast,
    AssociatedConstant,
    Closure,
    AsyncClosure,
    AsyncBlock,
    Await,
    Capture,
    Binding,
    HostAccess,
}

/// Checked input at a static expression's token range, without executable slots.
/// Bindings/captures use `related` ranges within the same expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFact {
    pub site: TokenRange,
    pub kind: FactKind,
    pub targets: Vec<DefinitionId>,
    pub types: Vec<Ty>,
    pub related: Vec<TokenRange>,
}

/// Authored body category, also used by exact source inspection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceBodyKind {
    Function,
    AsyncFunction,
    Constant,
    StaticInitializer,
    Native,
    Required,
}

/// Authored owner. Only static initializers carry tokens and checked local facts.
/// For `static val N: i32 = ((helper()));`, tokens cover `((helper()))`, and
/// the call fact names `helper` without retaining or traversing its body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceBody {
    pub declaration: DefinitionId,
    pub kind: SourceBodyKind,
    pub tokens: Vec<SourceToken>,
    pub initializer: Option<TokenRange>,
    pub facts: Vec<SourceFact>,
}

/// Static declaration whose own initializer expression is frozen across reload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceInitializer {
    pub declaration: DefinitionId,
}

/// Complete authored body inventory and static-only comparison roots.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SourceEvidence {
    pub bodies: Vec<SourceBody>,
    pub initializers: Vec<SourceInitializer>,
}

/// One bounded source collection, shared across a publication's modules.
pub struct SourceBudget<'a> {
    cancel: &'a CancellationToken,
    records: usize,
    bytes: usize,
}

impl<'a> SourceBudget<'a> {
    pub fn new(cancel: &'a CancellationToken) -> Self {
        Self {
            cancel,
            records: 1_000_000,
            bytes: 64 * 1024 * 1024,
        }
    }

    fn charge(&mut self, records: usize, bytes: usize) -> Result<(), DefinitionMappingError> {
        check_cancel(self.cancel)?;
        self.records = self
            .records
            .checked_sub(records)
            .ok_or(DefinitionMappingError::LimitExceeded)?;
        self.bytes = self
            .bytes
            .checked_sub(bytes)
            .ok_or(DefinitionMappingError::LimitExceeded)?;
        Ok(())
    }
}

struct BodyBuilder {
    owner: HirOwner,
    span: Span,
    token_spans: Vec<Span>,
    body: SourceBody,
}

impl BodyBuilder {
    fn site(&self, span: Span) -> TokenRange {
        TokenRange {
            start: self
                .token_spans
                .partition_point(|token| token.end <= span.start),
            end: self
                .token_spans
                .partition_point(|token| token.start < span.end),
        }
    }

    fn fact(
        &mut self,
        span: Span,
        kind: FactKind,
        targets: Vec<DefinitionId>,
        types: Vec<Ty>,
        budget: &mut SourceBudget<'_>,
    ) -> Result<(), DefinitionMappingError> {
        if targets.len() > 4096
            || types.len() > 4096
            || types.iter().any(|ty| !ty.within_wire_limits())
        {
            return Err(DefinitionMappingError::LimitExceeded);
        }
        budget.charge(1 + targets.len() + types.len(), 0)?;
        self.body.facts.push(SourceFact {
            site: self.site(span),
            kind,
            targets,
            types,
            related: vec![],
        });
        Ok(())
    }
}

fn definition(
    module: &CheckedAnalysis,
    name: ResolvedName,
) -> Result<DefinitionId, DefinitionMappingError> {
    match module.declarations.target(name).map(|item| &item.id) {
        Some(DeclarationId::Definition(id)) => Ok(*id),
        _ => Err(DefinitionMappingError::InvalidContract),
    }
}

fn provenance(
    module: &CheckedAnalysis,
    name: ResolvedName,
    kind: SourceBodyKind,
    budget: &mut SourceBudget<'_>,
) -> Result<SourceBody, DefinitionMappingError> {
    budget.charge(1, 0)?;
    Ok(SourceBody {
        declaration: definition(module, name)?,
        kind,
        tokens: vec![],
        initializer: None,
        facts: vec![],
    })
}

fn builder(
    module: &CheckedAnalysis,
    owner: BodyOwner,
    name: ResolvedName,
    span: Span,
    budget: &mut SourceBudget<'_>,
) -> Result<BodyBuilder, DefinitionMappingError> {
    let text = module
        .lowered
        .source
        .text()
        .get(span.start..span.end)
        .ok_or(DefinitionMappingError::InvalidContract)?;
    // Bound lexer allocation as well as retained spelling before lexing.
    budget.charge(1, text.len())?;
    let lexed = lex_with_cancellation(text, budget.cancel)
        .map_err(|_| DefinitionMappingError::Cancelled)?;
    let mut tokens = Vec::new();
    let mut token_spans = Vec::new();
    for token in lexed {
        if matches!(
            token.kind,
            TokenKind::Whitespace
                | TokenKind::LineComment
                | TokenKind::BlockComment
                | TokenKind::Eof
        ) {
            continue;
        }
        budget.charge(1, 0)?;
        tokens.push(SourceToken {
            kind: token.kind,
            text: text[token.span.start..token.span.end].to_owned(),
        });
        token_spans.push(Span {
            start: span.start + token.span.start,
            end: span.start + token.span.end,
        });
    }
    let initializer = TokenRange {
        start: 0,
        end: tokens.len(),
    };
    Ok(BodyBuilder {
        owner: HirOwner::Body(owner),
        span,
        token_spans,
        body: SourceBody {
            declaration: definition(module, name)?,
            kind: SourceBodyKind::StaticInitializer,
            tokens,
            initializer: Some(initializer),
            facts: vec![],
        },
    })
}

/// Collect authored identities and static expressions without following callees.
/// Missing identities/ranges, cancellation and budget exhaustion fail atomically.
pub fn collect(
    module: &CheckedAnalysis,
    budget: &mut SourceBudget<'_>,
) -> Result<SourceEvidence, DefinitionMappingError> {
    let hir = &module.lowered.module;
    let map = &module.lowered.source_map;
    let mut result = SourceEvidence::default();
    for function in &hir.functions {
        let typed = module
            .typed
            .functions
            .iter()
            .find(|typed| typed.id == function.id)
            .ok_or(DefinitionMappingError::InvalidContract)?;
        let kind = match &typed.implementation {
            FunctionImplementation::Native(_) => SourceBodyKind::Native,
            FunctionImplementation::Script if function.is_async => SourceBodyKind::AsyncFunction,
            FunctionImplementation::Script => SourceBodyKind::Function,
            FunctionImplementation::Required => SourceBodyKind::Required,
        };
        result.bodies.push(provenance(
            module,
            ResolvedName::Function(function.id),
            kind,
            budget,
        )?);
    }
    for item in &hir.consts {
        result.bodies.push(provenance(
            module,
            ResolvedName::Const(item.id),
            SourceBodyKind::Constant,
            budget,
        )?);
    }
    let mut builders = Vec::new();
    for item in &hir.statics {
        let initializer = map
            .static_initializer_span(item.id)
            .ok_or(DefinitionMappingError::InvalidContract)?;
        let body = builder(
            module,
            BodyOwner::Static(item.id),
            ResolvedName::Static(item.id),
            initializer,
            budget,
        )?;
        budget.charge(1, 0)?;
        result.initializers.push(SourceInitializer {
            declaration: body.body.declaration,
        });
        builders.push(body);
    }
    if !builders.is_empty() {
        let owners: HashMap<_, _> = builders
            .iter()
            .enumerate()
            .map(|(index, body)| (body.owner, index))
            .collect();
        facts::collect(module, &owners, &mut builders, budget)?;
    }
    result
        .bodies
        .extend(builders.into_iter().map(|body| body.body));
    result.canonical_order(module.definitions(), budget.cancel)?;
    Ok(result)
}

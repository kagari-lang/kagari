//! Checked source evidence retained before reachability, constant folding or specialization.
//!
//! These are source-facing records, not executable certificates. Token coordinates
//! are local to an authored declaration, never byte offsets or HIR arena indices.
//! Exact meaningful tokens plus checked resolutions conservatively distinguish
//! template computations; this deliberately does not prove algebraic equivalence.
//! The compiler maps these scoped references through declaration correspondence.

use crate::{
    CheckedAnalysis,
    declarations::DeclarationId,
    hir::ids::{BodyOwner, HirOwner},
    resolver::resolved::ResolvedName,
    typeck::FunctionImplementation,
    types::semantic::{lower_nominal_type, lower_type},
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
use kagari_types::ty::{GenericBound, GenericParam, NominalTy, Ty};
use std::collections::HashMap;

mod facts;
mod mapping;
mod order;
mod roots;
#[cfg(test)]
mod tests;

/// An exact non-trivia lexeme. String contents, escapes and raw delimiters survive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceToken {
    pub kind: TokenKind,
    pub text: String,
}

/// Half-open positions in a body's non-trivia token stream, not source offsets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TokenRange {
    pub start: usize,
    pub end: usize,
}

/// Why a checked operation has no complete finite script-target proof.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum UnknownDependency {
    FunctionValue,
    FutureResume,
    InterfaceDispatch,
    NativeReentry,
    RuntimeHelper,
    HostAccess,
    MissingCheckedFact,
}

/// Role of an ordered semantic fact attached to a token range.
///
/// `Call`/`FunctionValue`: targets name the callee, types are type arguments.
/// `TraitCall`/`Protocol`: targets name the member (when present), types begin
/// with receiver and applied interface, followed by method arguments.
/// `Signature`: parameter types followed by result. `Selection`: implementation
/// followed by selected member and authored default origin (when present); types
/// begin with receiver and applied interface, followed by applied arguments.
/// Other references retain source order, never allocation or hash-map order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum FactKind {
    Reference,
    Type,
    Call,
    FunctionValue,
    TraitCall,
    Signature,
    Protocol,
    Selection,
    Member,
    Construction,
    DeclaredCoercion,
    BoundCoercion,
    IntrinsicCoercion,
    InterfaceUpcast,
    AssociatedConstant,
    NativeRequirement,
    Closure,
    AsyncClosure,
    AsyncBlock,
    Await,
    Capture,
    Binding,
    HostAccess,
    Unknown(UnknownDependency),
}

/// A checked source dependency or semantic input; no executable slot is present.
/// Bindings/creations use `related` token ranges within this same body. Other
/// facts leave it empty. This keeps source-local IDs out of persistent evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFact {
    pub site: TokenRange,
    pub kind: FactKind,
    pub targets: Vec<DefinitionId>,
    pub types: Vec<Ty>,
    pub related: Vec<TokenRange>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceBodyKind {
    Function,
    AsyncFunction,
    Constant,
    StaticInitializer,
    Native,
    Required,
}

/// Authored template, including unused generics and named block-local functions.
/// Nested closure/async regions have creation facts; their computations remain in
/// this exact owner. Instance planning later associates emitted bodies with them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceBody {
    pub declaration: DefinitionId,
    pub kind: SourceBodyKind,
    pub parameters: Vec<GenericParam>,
    pub bounds: Vec<GenericBound>,
    pub tokens: Vec<SourceToken>,
    /// Only statics require an independently frozen full initializer expression.
    pub initializer: Option<TokenRange>,
    pub facts: Vec<SourceFact>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SourceRootKind {
    Hash,
    Equality,
    StaticInitializer,
}

/// Published protection root, independent of materialized applications or use.
/// Protocol roots retain their symbolic receiver/interface and selected members.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRoot {
    pub declaration: DefinitionId,
    pub kind: SourceRootKind,
    pub receiver: Option<Ty>,
    pub interface: Option<NominalTy>,
    pub members: Vec<DefinitionId>,
}

/// Complete source owner inventory, plus checked implementation/default edges.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SourceEvidence {
    pub bodies: Vec<SourceBody>,
    pub roots: Vec<SourceRoot>,
    pub selections: Vec<SourceFact>,
}

/// One bounded source collection. Final contract admission additionally charges
/// the combined historical envelope, wire bytes and executable records.
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

fn builder(
    module: &CheckedAnalysis,
    owner: BodyOwner,
    name: ResolvedName,
    span: Span,
    kind: SourceBodyKind,
    budget: &mut SourceBudget<'_>,
) -> Result<BodyBuilder, DefinitionMappingError> {
    let text = module
        .lowered
        .source
        .text()
        .get(span.start..span.end)
        .ok_or(DefinitionMappingError::InvalidContract)?;
    // Bound lexer allocation as well as the retained spelling before lexing.
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
    Ok(BodyBuilder {
        owner: HirOwner::Body(owner),
        span,
        token_spans,
        body: SourceBody {
            declaration: definition(module, name)?,
            kind,
            parameters: vec![],
            bounds: vec![],
            tokens,
            initializer: None,
            facts: vec![],
        },
    })
}

/// Collect all checked bodies without following executable reachability.
///
/// Missing required identities/ranges fail. Unknown dynamic/native dependencies
/// are explicit facts requiring conservative protection, not empty edge sets.
/// Cancellation or budget exhaustion returns no partial evidence.
pub fn collect(
    module: &CheckedAnalysis,
    budget: &mut SourceBudget<'_>,
) -> Result<SourceEvidence, DefinitionMappingError> {
    let hir = &module.lowered.module;
    let map = &module.lowered.source_map;
    let mut builders = Vec::new();
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
        let mut body = builder(
            module,
            BodyOwner::Function(function.id),
            ResolvedName::Function(function.id),
            map.function_span(function.id),
            kind,
            budget,
        )?;
        body.body.parameters = typed
            .generic_params
            .iter()
            .map(|parameter| GenericParam {
                owner: parameter.owner,
                position: parameter.position,
            })
            .collect();
        body.body.bounds = typed
            .bounds
            .iter()
            .map(|(ty, constraints)| GenericBound {
                ty: lower_type(ty),
                constraints: constraints.iter().map(lower_nominal_type).collect(),
            })
            .collect();
        body.fact(
            body.span,
            FactKind::Signature,
            vec![],
            typed
                .params
                .iter()
                .map(|param| lower_type(&param.ty))
                .chain([lower_type(&typed.return_type)])
                .collect(),
            budget,
        )?;
        if matches!(kind, SourceBodyKind::Native) {
            body.fact(
                body.span,
                FactKind::Unknown(UnknownDependency::NativeReentry),
                vec![body.body.declaration],
                vec![],
                budget,
            )?;
        }

        builders.push(body);
    }
    for item in &hir.consts {
        builders.push(builder(
            module,
            BodyOwner::Const(item.id),
            ResolvedName::Const(item.id),
            map.const_span(item.id),
            SourceBodyKind::Constant,
            budget,
        )?);
    }
    let mut result = SourceEvidence::default();
    for item in &hir.statics {
        let mut body = builder(
            module,
            BodyOwner::Static(item.id),
            ResolvedName::Static(item.id),
            map.static_span(item.id),
            SourceBodyKind::StaticInitializer,
            budget,
        )?;
        let initializer = map
            .static_initializer_span(item.id)
            .ok_or(DefinitionMappingError::InvalidContract)?;
        body.body.initializer = Some(body.site(initializer));
        budget.charge(1, 0)?;
        result.roots.push(SourceRoot {
            declaration: body.body.declaration,
            kind: SourceRootKind::StaticInitializer,
            receiver: None,
            interface: None,
            members: vec![],
        });
        builders.push(body);
    }
    let owners: HashMap<_, _> = builders
        .iter()
        .enumerate()
        .map(|(index, body)| (body.owner, index))
        .collect();
    facts::collect(module, &owners, &mut builders, budget)?;
    for native in module.lowered.registered_native_declarations() {
        if let Some(body) = builders
            .iter_mut()
            .find(|body| body.body.declaration == native.declaration)
        {
            for requirement in &native.callable_requirements {
                body.fact(
                    body.span,
                    FactKind::NativeRequirement,
                    vec![requirement.member],
                    [
                        requirement.receiver.clone(),
                        Ty::Trait(requirement.interface.clone()),
                    ]
                    .into_iter()
                    .chain(requirement.arguments.iter().cloned())
                    .collect(),
                    budget,
                )?;
            }
            if let Some(result) = &native.concrete_result {
                body.fact(
                    body.span,
                    FactKind::Construction,
                    vec![],
                    vec![result.clone(), native.function.return_type.clone()],
                    budget,
                )?;
            }
        } else {
            return Err(DefinitionMappingError::InvalidContract);
        }
    }
    roots::collect(module, &mut result, budget)?;
    result.bodies = builders.into_iter().map(|body| body.body).collect();
    result.canonical_order(module.definitions(), budget.cancel)?;
    Ok(result)
}

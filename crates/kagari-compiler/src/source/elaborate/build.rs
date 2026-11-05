//! Body-owned construction state and mandatory semantic-fact validation.

mod completion;
mod expr;
mod structure;

use crate::source::elaborate::{
    ElaborationError,
    model::{CheckedBody, CheckedCapture, CheckedExpr, CheckedExprKind, OperationId, ValueBinding},
};
use kagari_common::{cancellation::CancellationToken, span::Span};
use kagari_hir::{
    compile_input::{
        body::{BodyRoot, CheckedBodyInput, bindings::BindingId, flow::BodyCompletion},
        environment::CheckedEnvironment,
    },
    hir::ids::ExprId,
    resolver::resolved::ResolvedName,
    types::TypeId,
};

pub(super) struct Builder<'a, 'c> {
    input: CheckedBodyInput<'a>,
    environment: CheckedEnvironment<'a>,
    completion: BodyCompletion<'a>,
    cancel: &'c CancellationToken,
    body: CheckedBody,
}

impl<'a, 'c> Builder<'a, 'c> {
    pub(super) fn new(input: CheckedBodyInput<'a>, cancel: &'c CancellationToken) -> Self {
        Self {
            input,
            environment: input.environment(),
            completion: input.completion(),
            cancel,
            body: CheckedBody {
                key: input.key(),
                root: input.root(),
                captures: Vec::new(),
                cell_locals: Default::default(),
                operations: Vec::new(),
                values: Default::default(),
                results: Default::default(),
                blocks: Default::default(),
                statements: Default::default(),
                places: Default::default(),
                patterns: Default::default(),
            },
        }
    }

    pub(super) fn finish(mut self) -> Result<CheckedBody, ElaborationError> {
        self.check()?;
        if let Some(closure) = self.input.key().closure {
            self.body.captures = self.captures(closure)?;
        }
        match self.input.root() {
            BodyRoot::Block(id) => self.block(id)?,
            BodyRoot::Expression(id) => {
                self.expression(id)?;
            }
        }
        self.check()?;
        Ok(self.body)
    }

    fn captures(&mut self, closure: ExprId) -> Result<Vec<CheckedCapture>, ElaborationError> {
        let captures = self.require(
            self.input
                .captures(closure, self.cancel)
                .map_err(|_| ElaborationError::Cancelled)?,
            self.input.expression_span(closure),
            "missing captured binding contract",
        )?;
        let mut result = Vec::with_capacity(captures.len());
        for capture in captures {
            self.check()?;
            let ty = self.valid_type(capture.ty, Some(capture.span))?;
            let by_cell =
                matches!(capture.id, BindingId::Local(_)) && capture.writeability.is_var();
            if let BindingId::Local(id) = capture.id
                && by_cell
            {
                self.body.cell_locals.insert(id);
            }
            result.push(CheckedCapture {
                id: capture.id,
                name: capture.name.to_owned(),
                span: capture.span,
                ty,
                by_cell,
            });
        }
        Ok(result)
    }

    fn value_binding(
        &self,
        resolved: &ResolvedName,
        span: Span,
    ) -> Result<ValueBinding, ElaborationError> {
        if let Some(binding) = BindingId::from_resolution(resolved) {
            self.require(
                self.input.binding(binding),
                Some(span),
                "missing lexical binding contract",
            )?;
            return Ok(ValueBinding::Lexical(binding));
        }
        let declaration = self.require(
            self.environment.static_declaration(resolved),
            Some(span),
            "name is not a checked runtime value location",
        )?;
        Ok(ValueBinding::Static(declaration))
    }

    fn completes(&mut self, id: ExprId) -> Result<bool, ElaborationError> {
        let result = self
            .completion
            .expression(id, self.cancel)
            .map_err(|_| ElaborationError::Cancelled)?;
        self.require(
            result,
            self.input.expression_span(id),
            "foreign completion expression",
        )
    }

    fn check(&self) -> Result<(), ElaborationError> {
        self.cancel.check().map_err(|_| ElaborationError::Cancelled)
    }

    fn error(&self, span: Option<Span>, reason: &'static str) -> ElaborationError {
        ElaborationError::InvalidInput { span, reason }
    }

    fn require<T>(
        &self,
        value: Option<T>,
        span: Option<Span>,
        reason: &'static str,
    ) -> Result<T, ElaborationError> {
        value.ok_or_else(|| self.error(span, reason))
    }

    fn valid_type(&self, ty: TypeId, span: Option<Span>) -> Result<TypeId, ElaborationError> {
        if ty.is_unresolved() {
            Err(self.error(span, "unresolved type in executable operation"))
        } else {
            Ok(ty)
        }
    }

    fn value_type(&self, id: ExprId) -> Result<TypeId, ElaborationError> {
        let span = self.input.expression_span(id);
        let facts = self.require(self.input.expression_facts(id), span, "foreign expression")?;
        self.valid_type(
            self.require(facts.ty, span, "missing expression type")?,
            span,
        )
    }

    fn append(
        &mut self,
        source: ExprId,
        span: Span,
        ty: TypeId,
        kind: CheckedExprKind,
    ) -> OperationId {
        let id = OperationId(self.body.operations.len());
        self.body.operations.push(CheckedExpr {
            source,
            span,
            ty,
            kind,
        });
        id
    }
}

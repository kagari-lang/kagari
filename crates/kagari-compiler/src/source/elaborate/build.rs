//! Body-owned construction state and mandatory semantic-fact validation.

mod expr;
mod structure;

use crate::source::elaborate::{
    ElaborationError,
    model::{CheckedBody, CheckedExpr, CheckedExprKind, OperationId},
};
use kagari_common::{cancellation::CancellationToken, span::Span};
use kagari_hir::{
    compile_input::{
        body::{BodyRoot, CheckedBodyInput},
        environment::CheckedEnvironment,
    },
    hir::ids::ExprId,
    types::TypeId,
};

pub(super) struct Builder<'a, 'c> {
    input: CheckedBodyInput<'a>,
    environment: CheckedEnvironment<'a>,
    cancel: &'c CancellationToken,
    body: CheckedBody,
}

impl<'a, 'c> Builder<'a, 'c> {
    pub(super) fn new(input: CheckedBodyInput<'a>, cancel: &'c CancellationToken) -> Self {
        Self {
            input,
            environment: input.environment(),
            cancel,
            body: CheckedBody {
                key: input.key(),
                root: input.root(),
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
        match self.input.root() {
            BodyRoot::Block(id) => self.block(id)?,
            BodyRoot::Expression(id) => {
                self.expression(id)?;
            }
        }
        self.check()?;
        Ok(self.body)
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

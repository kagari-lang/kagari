//! Structural control flow and mutation targets, with spelling-free member selections.

use crate::source::elaborate::{
    ElaborationError,
    build::Builder,
    model::{
        CheckedBlock, CheckedLocal, CheckedPattern, CheckedPatternKind, CheckedPlace,
        CheckedPlaceKind, CheckedStmt, CheckedStmtKind, IndexWrite,
    },
};
use kagari_common::span::Span;
use kagari_hir::hir::{
    expr::Condition,
    ids::{BlockId, LocalId, PatternId, PlaceId, StmtId},
    pattern::PatternKind,
    place::PlaceKind,
    stmt::StmtKind,
};
use kagari_hir::types::TypeId;
use kagari_types::scalar::BuiltinType;

impl Builder<'_, '_> {
    pub(super) fn block(&mut self, id: BlockId) -> Result<(), ElaborationError> {
        self.check()?;
        if self.body.blocks.contains_key(&id) {
            return Ok(());
        }
        let block = self
            .require(self.input.block(id), None, "foreign block")?
            .clone();
        let span = self.require(self.input.block_span(id), None, "missing block origin")?;
        let mut statements = Vec::new();
        let mut terminated = false;
        for statement in block.statements {
            self.statement(statement)?;
            statements.push(statement);
            let normal = self
                .completion
                .statement(statement, self.cancel)
                .map_err(|_| ElaborationError::Cancelled)?;
            if !self.require(normal, Some(span), "foreign completion statement")? {
                terminated = true;
                break;
            }
        }
        let tail = block.tail_expr.filter(|_| !terminated);
        if let Some(tail) = tail {
            self.expression(tail)?;
        }
        self.body
            .blocks
            .insert(id, CheckedBlock { statements, tail });
        Ok(())
    }

    fn statement(&mut self, id: StmtId) -> Result<(), ElaborationError> {
        self.check()?;
        let span = self.require(self.input.statement_span(id), None, "foreign statement")?;
        let node = self
            .require(self.input.statement(id), Some(span), "missing statement")?
            .clone();
        let kind = match node.kind {
            StmtKind::Function(_) => CheckedStmtKind::Function,
            StmtKind::Binding {
                pattern,
                initializer,
                ..
            } => {
                self.expression(initializer)?;
                if self.completes(initializer)? {
                    self.pattern(pattern)?;
                    CheckedStmtKind::Binding {
                        pattern,
                        initializer,
                    }
                } else {
                    CheckedStmtKind::Expr(initializer)
                }
            }
            StmtKind::Assign { target, op, value } => {
                self.place(target)?;
                if self.place_completes(target)? {
                    self.expression(value)?;
                    if self.completes(value)? {
                        CheckedStmtKind::Assign { target, op, value }
                    } else {
                        CheckedStmtKind::EvaluateAssignment { target, value }
                    }
                } else {
                    CheckedStmtKind::EvaluatePlace(target)
                }
            }
            StmtKind::Return { expr } => {
                if let Some(expr) = expr {
                    self.expression(expr)?;
                }
                CheckedStmtKind::Return(expr)
            }
            StmtKind::While { condition, body } => {
                self.expression(condition.value())?;
                if self.completes(condition.value())? {
                    self.condition(&condition)?;
                    self.block(body)?;
                    CheckedStmtKind::While { condition, body }
                } else {
                    CheckedStmtKind::Expr(condition.value())
                }
            }
            StmtKind::Loop { body } => {
                self.block(body)?;
                CheckedStmtKind::Loop(body)
            }
            StmtKind::For {
                pattern,
                iterable,
                body,
            } => {
                self.expression(iterable)?;
                if !self.completes(iterable)? {
                    CheckedStmtKind::Expr(iterable)
                } else {
                    let iteration = self
                        .require(
                            self.input
                                .expression_facts(iterable)
                                .and_then(|facts| facts.iteration),
                            Some(span),
                            "missing iteration contract",
                        )?
                        .clone();
                    self.pattern(pattern)?;
                    self.block(body)?;
                    CheckedStmtKind::For {
                        pattern,
                        iterable,
                        body,
                        iteration: Box::new(iteration),
                    }
                }
            }
            StmtKind::Break => CheckedStmtKind::Break,
            StmtKind::BreakValue(expr) => {
                self.expression(expr)?;
                CheckedStmtKind::BreakValue(expr)
            }
            StmtKind::Continue => CheckedStmtKind::Continue,
            StmtKind::Expr(expr) => {
                self.expression(expr)?;
                CheckedStmtKind::Expr(expr)
            }
        };
        self.body.statements.insert(id, CheckedStmt { span, kind });
        Ok(())
    }

    pub(super) fn condition(&mut self, condition: &Condition) -> Result<(), ElaborationError> {
        self.expression(condition.value())?;
        if let Condition::Binding { pattern, .. } = condition {
            self.pattern(*pattern)?;
        }
        Ok(())
    }

    pub(super) fn local(
        &self,
        id: LocalId,
        name: String,
        span: Span,
    ) -> Result<CheckedLocal, ElaborationError> {
        let ty = self.require(self.input.local_type(id), Some(span), "missing local type")?;
        Ok(CheckedLocal {
            id,
            span: self.require(
                self.input.local_span(id),
                Some(span),
                "missing local origin",
            )?,
            name,
            ty: self.valid_type(ty, Some(span))?,
        })
    }

    pub(super) fn pattern(&mut self, id: PatternId) -> Result<(), ElaborationError> {
        self.check()?;
        if self.body.patterns.contains_key(&id) {
            return Ok(());
        }
        let span = self.require(self.input.pattern_span(id), None, "foreign pattern")?;
        let node = self
            .require(self.input.pattern(id), Some(span), "missing pattern")?
            .clone();
        let facts = self.require(
            self.input.pattern_facts(id),
            Some(span),
            "missing pattern facts",
        )?;
        let kind = if let Some(scalar) = facts.scalar {
            CheckedPatternKind::Constant(scalar.clone())
        } else if let Some(variant) = facts.variant {
            let fields = match node.kind {
                PatternKind::Name { .. } => Vec::new(),
                PatternKind::EnumVariant { fields, .. } => fields,
                _ => return Err(self.error(Some(span), "variant on non-constructor pattern")),
            };
            for field in &fields {
                self.pattern(*field)?;
            }
            CheckedPatternKind::Variant {
                declaration: *variant,
                fields,
            }
        } else {
            match node.kind {
                PatternKind::Wildcard => CheckedPatternKind::Wildcard,
                PatternKind::Name { local, name } => {
                    CheckedPatternKind::Binding(self.local(local, name, span)?)
                }
                PatternKind::Range { inclusive, .. } => {
                    let (start, end) =
                        self.require(facts.range, Some(span), "missing checked range endpoints")?;
                    CheckedPatternKind::Range {
                        start: start.clone(),
                        end: end.clone(),
                        inclusive,
                    }
                }
                PatternKind::Or(alternatives) => {
                    for alternative in &alternatives {
                        self.pattern(*alternative)?;
                    }
                    CheckedPatternKind::Or(alternatives)
                }
                PatternKind::Tuple(elements) => {
                    for element in &elements {
                        self.pattern(*element)?;
                    }
                    CheckedPatternKind::Tuple(elements)
                }
                PatternKind::Struct { fields, .. } => {
                    let selected =
                        self.require(facts.fields, Some(span), "missing pattern fields")?;
                    if selected.len() != fields.len() {
                        return Err(self.error(Some(span), "pattern field count mismatch"));
                    }
                    let mut checked = Vec::with_capacity(fields.len());
                    for (field, selected) in fields.into_iter().zip(selected) {
                        self.pattern(field.pattern)?;
                        checked.push((*selected, field.pattern));
                    }
                    CheckedPatternKind::Struct(checked)
                }
                PatternKind::EnumVariant { .. } | PatternKind::Literal(_) => {
                    return Err(self.error(Some(span), "missing checked pattern selection"));
                }
            }
        };
        self.body.patterns.insert(
            id,
            CheckedPattern {
                irrefutable: facts.irrefutable,
                kind,
            },
        );
        Ok(())
    }

    fn place_completes(&mut self, id: PlaceId) -> Result<bool, ElaborationError> {
        let result = self
            .completion
            .place(id, self.cancel)
            .map_err(|_| ElaborationError::Cancelled)?;
        self.require(
            result,
            self.input.place_span(id),
            "foreign completion place",
        )
    }

    fn place(&mut self, id: PlaceId) -> Result<(), ElaborationError> {
        self.check()?;
        if self.body.places.contains_key(&id) {
            return Ok(());
        }
        let span = self.require(self.input.place_span(id), None, "foreign mutation target")?;
        let facts = self.require(
            self.input.place_facts(id),
            Some(span),
            "missing mutation facts",
        )?;
        if !self.place_completes(id)? {
            let (base, values) = if let Some(host) = facts.host_path {
                if host.root == id {
                    return Err(self.error(Some(span), "recursive host mutation root"));
                }
                (
                    Some(host.root),
                    host.dynamic_arguments
                        .iter()
                        .map(|(_, id)| *id)
                        .collect::<Vec<_>>(),
                )
            } else {
                match &self
                    .require(self.input.place(id), Some(span), "foreign mutation target")?
                    .kind
                {
                    PlaceKind::Expr(value) => (None, vec![*value]),
                    PlaceKind::Field { base, .. } => (Some(*base), Vec::new()),
                    PlaceKind::Index { base, index } => (Some(*base), vec![*index]),
                    PlaceKind::Name(_) => {
                        return Err(
                            self.error(Some(span), "named place cannot terminate preparation")
                        );
                    }
                }
            };
            let mut evaluated = Vec::new();
            let mut normal = true;
            if let Some(base) = base {
                self.place(base)?;
                normal = self.place_completes(base)?;
            }
            if normal {
                for value in values {
                    self.expression(value)?;
                    evaluated.push(value);
                    if !self.completes(value)? {
                        break;
                    }
                }
            }
            self.body.places.insert(
                id,
                CheckedPlace {
                    ty: TypeId::Builtin(BuiltinType::Never),
                    kind: CheckedPlaceKind::Diverging {
                        base,
                        read_base: facts.host_path.is_some()
                            || (facts.index_read.is_some() && facts.index_write.is_none()),
                        values: evaluated,
                    },
                },
            );
            return Ok(());
        }
        let ty = self.valid_type(
            self.require(facts.ty, Some(span), "missing place type")?,
            Some(span),
        )?;
        let kind = if let Some(host) = facts.host_path {
            if host.root == id {
                return Err(self.error(Some(span), "recursive host mutation root"));
            }
            self.place(host.root)?;
            for (_, expr) in &host.dynamic_arguments {
                self.expression(*expr)?;
            }
            CheckedPlaceKind::Host(host.clone())
        } else {
            let node = self
                .require(self.input.place(id), Some(span), "missing mutation target")?
                .clone();
            match node.kind {
                PlaceKind::Name(_) => {
                    let binding =
                        self.require(facts.binding, Some(span), "missing mutation binding")?;
                    CheckedPlaceKind::Binding(self.value_binding(&binding, span)?)
                }
                PlaceKind::Expr(expr) => {
                    self.expression(expr)?;
                    CheckedPlaceKind::Value(expr)
                }
                PlaceKind::Field { base, .. } => {
                    self.place(base)?;
                    CheckedPlaceKind::Field {
                        base,
                        field: *self.require(facts.field, Some(span), "missing mutation field")?,
                    }
                }
                PlaceKind::Index { base, index } => {
                    self.place(base)?;
                    self.expression(index)?;
                    let operation = if let Some((write, setter)) = facts.index_write {
                        IndexWrite::Method {
                            read: self
                                .require(
                                    facts.index_read,
                                    Some(span),
                                    "missing index read contract",
                                )?
                                .clone(),
                            write: write.clone(),
                            setter: *setter,
                        }
                    } else if let Some(read) = facts.index_read {
                        IndexWrite::ReadMethod(read.clone())
                    } else {
                        IndexWrite::Aggregate
                    };
                    CheckedPlaceKind::Index {
                        base,
                        index,
                        operation,
                    }
                }
            }
        };
        self.body.places.insert(id, CheckedPlace { ty, kind });
        Ok(())
    }
}

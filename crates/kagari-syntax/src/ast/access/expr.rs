//! Expression access follows authored fields and ordered string/list alternatives.

use crate::{
    ast::{
        borrow::Ast,
        expr::{
            ArrayContents, ArrayExpr, AsyncBlockExpr, AwaitExpr, BinaryExpr, BinaryOp,
            BindingCondition, BlockExpr, CallExpr, CastExpr, ClosureExpr, ClosureParam,
            ClosureParams, Condition, FieldExpr, FieldInit, FieldInitList, IfExpr, IndexExpr,
            InterpolatedString, Interpolation, Literal, LoopExpr, MatchArm, MatchArmList,
            MatchExpr, ParenExpr, PathExpr, PrefixExpr, PrefixOp, PropagateExpr, RangeExpr,
            RangeOp, StringPart, StructEntry, StructExpr, StructUpdateBase, TupleExpr,
        },
        misc::{GenericArgList, Name, Path},
        ty::{QualifiedType, TypeSyntax},
        views::{ExprView, StmtView, StringPartView},
    },
    kind::SyntaxKind,
};

impl<'a> Ast<'a, BlockExpr> {
    pub fn statements(self) -> impl Iterator<Item = StmtView<'a>> {
        self.node
            .statements
            .iter()
            .map(move |stmt| self.with(stmt).view())
    }

    pub fn tail_expr(self) -> Option<ExprView<'a>> {
        self.node.tail.as_ref().map(|expr| self.with(expr).view())
    }
}

impl<'a> Ast<'a, PathExpr> {
    pub fn path(self) -> Option<Ast<'a, Path>> {
        match self.node {
            PathExpr::Plain(path) => Some(self.with(path)),
            PathExpr::Applied(path) => path.path.as_ref().map(|node| self.with(node)),
            PathExpr::Qualified(_) => None,
        }
    }

    pub fn generic_args(self) -> Option<Ast<'a, GenericArgList>> {
        match self.node {
            PathExpr::Applied(path) => Some(self.with(&path.arguments)),
            PathExpr::Plain(_) | PathExpr::Qualified(_) => None,
        }
    }

    pub fn qualified_type(self) -> Option<Ast<'a, QualifiedType>> {
        match self.node {
            PathExpr::Qualified(ty) => Some(self.with::<QualifiedType>(ty)),
            PathExpr::Plain(_) | PathExpr::Applied(_) => None,
        }
    }

    pub fn name(self) -> Option<Ast<'a, Name>> {
        match self.node {
            PathExpr::Applied(path) => path
                .member
                .as_ref()?
                .name
                .as_ref()
                .map(|name| self.with(name)),
            PathExpr::Plain(_) | PathExpr::Qualified(_) => None,
        }
    }

    pub fn name_text(self) -> Option<String> {
        if let Some(qualified) = self.qualified_type() {
            return qualified.member()?.text();
        }
        let mut path = self.path()?.text()?;
        if let Some(name) = self.name() {
            path.push_str("::");
            path.push_str(&name.text()?);
        }
        Some(path)
    }
}

impl Ast<'_, Literal> {
    /// Copy the literal's source spelling, including delimiters and escapes.
    /// This does not decode its value; use `source_text()` to borrow the spelling.
    pub fn text(self) -> Option<String> {
        Some(self.source_text().to_owned())
    }

    pub fn kind(self) -> Option<SyntaxKind> {
        Some(
            self.owner
                .lexical
                .kind(self.node.token.index() as usize)
                .to_syntax_kind(),
        )
    }
}

impl<'a> Ast<'a, ParenExpr> {
    required_expression!(expr, expression);
}

impl<'a> Ast<'a, PropagateExpr> {
    required_expression!(expr, expression);
}

impl<'a> Ast<'a, AwaitExpr> {
    required_expression!(expr, expression);
}

impl<'a> Ast<'a, PrefixExpr> {
    required_expression!(expr, expression);

    pub fn operator(self) -> Option<PrefixOp> {
        Some(self.node.operator.as_ref()?.operation)
    }
}

impl<'a> Ast<'a, BinaryExpr> {
    required_expression!(lhs, lhs);
    required_expression!(rhs, rhs);

    pub fn operator(self) -> Option<BinaryOp> {
        Some(self.node.operator.as_ref()?.operation)
    }
}

impl<'a> Ast<'a, CastExpr> {
    required_expression!(expr, expression);
    required_child!(ty, TypeSyntax, ty);
}

impl<'a> Ast<'a, RangeExpr> {
    pub fn start(self) -> Option<ExprView<'a>> {
        self.node.start.as_ref().map(|expr| self.with(expr).view())
    }

    pub fn end(self) -> Option<ExprView<'a>> {
        self.node
            .end
            .as_ref()?
            .as_ref()
            .map(|expr| self.with(expr).view())
    }

    pub fn inclusive(self) -> bool {
        self.node.operator.operation == RangeOp::Inclusive
    }
}

impl<'a> Ast<'a, CallExpr> {
    required_expression!(callee, callee);

    pub fn generic_args(self) -> Option<Ast<'a, GenericArgList>> {
        Some(self.with(&self.node.application.as_ref()?.arguments))
    }

    /// Present arguments in authored order, excluding missing slots and punctuation.
    /// For `f(a,,b,)` this yields `a, b`; inspect `data().arguments.entries` to keep
    /// the missing middle entry and trailing comma. See [`SeparatedList`](crate::ast::site::SeparatedList).
    ///
    /// ```
    /// use kagari_source::source::SourceFile;
    /// use kagari_syntax::{
    ///     ast::{site::Required, views::{ExprView, ItemView}},
    ///     parser::{ParseOptions, parse},
    /// };
    /// let source = SourceFile::new("recovery.kgr", "fn f() { f(a,,b,) }");
    /// let parsed = parse(&source, ParseOptions::default(), &Default::default())?;
    /// assert!(!parsed.diagnostics().is_empty());
    /// let root = parsed.module();
    /// let ItemView::FnDef(function) = root.items().next().unwrap() else { panic!() };
    /// let ExprView::CallExpr(call) = function.body().unwrap().tail_expr().unwrap()
    ///     else { panic!() };
    /// assert_eq!(call.args().count(), 2);
    /// let entries = &call.data().arguments.entries;
    /// assert_eq!(entries.len(), 3);
    /// assert!(matches!(entries[1].element, Required::Missing { .. }));
    /// # Ok::<(), kagari_common::cancellation::Cancelled>(())
    /// ```
    pub fn args(self) -> impl Iterator<Item = ExprView<'a>> {
        self.list(&self.node.arguments).map(|expr| expr.view())
    }
}

impl<'a> Ast<'a, FieldExpr> {
    required_expression!(receiver, receiver);
    required_child!(name, Name, name);

    pub fn name_text(self) -> Option<String> {
        self.name()?.text()
    }
}

impl<'a> Ast<'a, IndexExpr> {
    required_expression!(receiver, receiver);
    required_expression!(index, index);
}

impl<'a> Ast<'a, IfExpr> {
    pub fn condition(self) -> Option<ExprView<'a>> {
        match self.node.condition.as_ref()? {
            Condition::Expression(expr) => Some(self.with(expr).view()),
            Condition::Binding(_) => None,
        }
    }

    pub fn binding_condition(self) -> Option<Ast<'a, BindingCondition>> {
        match self.node.condition.as_ref()? {
            Condition::Binding(binding) => Some(self.with::<BindingCondition>(binding)),
            Condition::Expression(_) => None,
        }
    }

    required_child!(then_branch, BlockExpr, then_body);

    pub fn else_branch(self) -> Option<ExprView<'a>> {
        self.node
            .else_clause
            .as_ref()?
            .body
            .as_ref()
            .map(|expr| self.with(expr).view())
    }
}

impl<'a> Ast<'a, BindingCondition> {
    required_child!(pattern, crate::ast::expr::Pattern, pattern);
    required_expression!(initializer, initializer);
}

impl<'a> Ast<'a, StructExpr> {
    required_child!(path, PathExpr, path);
    optional_child!(generic_args, GenericArgList, arguments);

    pub fn field_list(self) -> Ast<'a, FieldInitList> {
        self.with(&self.node.fields)
    }
}

impl<'a> Ast<'a, FieldInitList> {
    pub fn fields(self) -> impl Iterator<Item = Ast<'a, FieldInit>> {
        self.list(&self.node.entries)
            .filter_map(|entry| match entry.node {
                StructEntry::Field(field) => Some(entry.with(field)),
                StructEntry::Update(_) => None,
            })
    }

    pub fn update_base(self) -> Option<Ast<'a, StructUpdateBase>> {
        self.list(&self.node.entries)
            .find_map(|entry| match entry.node {
                StructEntry::Update(update) => Some(entry.with(update)),
                StructEntry::Field(_) => None,
            })
    }
}

impl<'a> Ast<'a, FieldInit> {
    required_child!(name, Name, name);

    pub fn name_text(self) -> Option<String> {
        self.name()?.text()
    }

    pub fn value(self) -> Option<ExprView<'a>> {
        self.node
            .value
            .as_ref()?
            .expression
            .as_ref()
            .map(|expr| self.with(expr).view())
    }
}

impl<'a> Ast<'a, StructUpdateBase> {
    required_expression!(value, value);
}

impl<'a> Ast<'a, MatchExpr> {
    required_expression!(scrutinee, scrutinee);

    pub fn arms(self) -> Ast<'a, MatchArmList> {
        self.with(&self.node.arms)
    }
}

impl<'a> Ast<'a, MatchArmList> {
    pub fn arms(self) -> impl Iterator<Item = Ast<'a, MatchArm>> {
        self.list(&self.node.arms)
    }
}

impl<'a> Ast<'a, MatchArm> {
    required_child!(pattern, crate::ast::expr::Pattern, pattern);
    required_expression!(expr, expression);

    pub fn guard(self) -> Option<ExprView<'a>> {
        self.node
            .guard
            .as_ref()?
            .expression
            .as_ref()
            .map(|expr| self.with(expr).view())
    }
}

impl<'a> Ast<'a, LoopExpr> {
    required_child!(body, BlockExpr, body);
}

impl<'a> Ast<'a, AsyncBlockExpr> {
    required_child!(body, BlockExpr, body);
}

impl<'a> Ast<'a, ClosureExpr> {
    required_expression!(body, body);

    pub fn is_async(self) -> bool {
        self.node.async_keyword.is_some()
    }

    pub fn params(self) -> impl Iterator<Item = Ast<'a, ClosureParam>> {
        let entries = match self.node.parameters.as_ref() {
            Some(ClosureParams::List(list)) => list.parameters.entries.as_ref(),
            Some(ClosureParams::Empty(_)) | None => &[],
        };
        entries
            .iter()
            .filter_map(move |entry| entry.element.as_ref().map(|param| self.with(param)))
    }
}

impl<'a> Ast<'a, ClosureParam> {
    required_child!(name, Name, name);

    pub fn ty(self) -> Option<Ast<'a, TypeSyntax>> {
        self.node
            .annotation
            .as_ref()?
            .ty
            .as_ref()
            .map(|ty| self.with(ty))
    }
}

impl<'a> Ast<'a, TupleExpr> {
    pub fn elements(self) -> impl Iterator<Item = ExprView<'a>> {
        self.list(&self.node.elements).map(|expr| expr.view())
    }
}

impl<'a> Ast<'a, ArrayExpr> {
    pub fn is_repeat(self) -> bool {
        matches!(self.node.contents, ArrayContents::Repeat { .. })
    }

    pub fn elements(self) -> impl Iterator<Item = ExprView<'a>> {
        let (entries, repeat) = match &self.node.contents {
            ArrayContents::List(elements) => (Some(elements), None),
            ArrayContents::Repeat { value, count, .. } => (None, Some([value, count])),
        };
        entries
            .into_iter()
            .flat_map(move |list| self.list(list))
            .chain(
                repeat
                    .into_iter()
                    .flatten()
                    .filter_map(move |value| value.as_ref().map(|expr| self.with(expr))),
            )
            .map(|expr| expr.view())
    }
}

impl<'a> Ast<'a, InterpolatedString> {
    pub fn parts(self) -> impl Iterator<Item = StringPartView<'a>> {
        self.node.parts.iter().map(move |part| match part {
            StringPart::Text(text) => StringPartView::Text(self.with(text)),
            StringPart::Hole(hole) => StringPartView::Hole(self.with(hole)),
        })
    }
}

impl<'a> Ast<'a, Interpolation> {
    required_child!(path, PathExpr, expression);

    pub fn debug(self) -> bool {
        self.node.debug.is_some()
    }
}

//! Statement fields are independent from recognized/recovered expression order.

use crate::ast::{
    borrow::Ast,
    expr::{BindingCondition, BlockExpr, Condition, Pattern},
    misc::Writeability,
    stmt::{
        AssignStmt, AssignmentOp, BindingStmt, BreakStmt, ExprStmt, ForStmt, LoopStmt, ReturnStmt,
        WhileStmt,
    },
    ty::TypeSyntax,
    views::ExprView,
};

impl<'a> Ast<'a, BindingStmt> {
    required_child!(pattern, Pattern, pattern);

    pub fn writeability(self) -> Option<Writeability> {
        Some(self.node.binding.as_ref()?.writeability)
    }

    pub fn is_val(self) -> bool {
        self.writeability() == Some(Writeability::Val)
    }

    pub fn is_var(self) -> bool {
        self.writeability() == Some(Writeability::Var)
    }

    pub fn ty(self) -> Option<Ast<'a, TypeSyntax>> {
        self.node
            .annotation
            .as_ref()?
            .ty
            .as_ref()
            .map(|ty| self.with(ty))
    }

    pub fn initializer(self) -> Option<ExprView<'a>> {
        self.node
            .initializer
            .as_ref()?
            .expression
            .as_ref()
            .map(|expr| self.with(expr).view())
    }
}

impl<'a> Ast<'a, AssignStmt> {
    required_expression!(target, target);
    required_expression!(value, value);

    pub fn operator(self) -> Option<AssignmentOp> {
        Some(self.node.operator.as_ref()?.operation)
    }
}

impl<'a> Ast<'a, ExprStmt> {
    required_expression!(expr, expression);
}

impl<'a> Ast<'a, ReturnStmt> {
    pub fn expr(self) -> Option<ExprView<'a>> {
        self.node.value.as_ref().map(|expr| self.with(expr).view())
    }
}

impl<'a> Ast<'a, BreakStmt> {
    pub fn expr(self) -> Option<ExprView<'a>> {
        self.node.value.as_ref().map(|expr| self.with(expr).view())
    }
}

impl<'a> Ast<'a, WhileStmt> {
    required_child!(body, BlockExpr, body);

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
}

impl<'a> Ast<'a, LoopStmt> {
    required_child!(body, BlockExpr, body);
}

impl<'a> Ast<'a, ForStmt> {
    required_child!(pattern, Pattern, pattern);
    required_child!(body, BlockExpr, body);
    required_expression!(iterable, iterable);
}

//! Traversal of explicit sum alternatives and punctuation-only field groups.

use std::sync::Arc;

use crate::{
    ast::{
        borrow::{Ast, NodeData},
        expr, item, misc,
        site::SyntaxSite,
        stmt, ty,
        visit::{ElementRef, FieldContext, Fields, NodeRef, Walk},
    },
    parser::ParsedFile,
};

macro_rules! alternative_walk {
    ($ty:ty { $($variant:ident),+ $(,)? }) => {
        impl Walk for $ty {
            fn walk<'a>(&'a self, context: FieldContext<'a>, output: &mut dyn FnMut(ElementRef<'a>)) {
                match self { $(Self::$variant(node) => node.walk(context, output),)+ }
            }
        }
    };
}

macro_rules! recovered_walk {
    ($ty:ty { $($variant:ident),+ $(,)? }) => {
        impl Walk for $ty {
            fn walk<'a>(&'a self, context: FieldContext<'a>, output: &mut dyn FnMut(ElementRef<'a>)) {
                match self {
                    $(Self::$variant(node) => node.walk(context, output),)+
                    Self::Error(site) => site.walk(context, output),
                }
            }
        }
    };
}

recovered_walk!(expr::Expr {
    BlockExpr,
    PathExpr,
    Literal,
    ParenExpr,
    PrefixExpr,
    CastExpr,
    PropagateExpr,
    AwaitExpr,
    BinaryExpr,
    RangeExpr,
    CallExpr,
    FieldExpr,
    IndexExpr,
    IfExpr,
    StructExpr,
    MatchExpr,
    LoopExpr,
    ClosureExpr,
    AsyncBlockExpr,
    TupleExpr,
    ArrayExpr,
    InterpolatedString,
});

recovered_walk!(item::Item {
    ModuleDef,
    UseDecl,
    FnDef,
    ConstDef,
    StaticDef,
    StructDef,
    EnumDef,
    TraitDef,
    ImplBlock,
    TypeAlias,
    OpaqueTypeDef
});
recovered_walk!(item::Member {
    MethodDef,
    AssociatedType,
    ConstDef
});
recovered_walk!(stmt::Stmt {
    BindingStmt,
    ReturnStmt,
    ExprStmt,
    AssignStmt,
    WhileStmt,
    LoopStmt,
    ForStmt,
    BreakStmt,
    ContinueStmt,
    FnDef
});
alternative_walk!(expr::Condition {
    Expression,
    Binding
});
alternative_walk!(expr::StructEntry { Field, Update });
alternative_walk!(expr::ClosureParams { Empty, List });
alternative_walk!(expr::StringPart { Text, Hole });
alternative_walk!(expr::PatternBound { Literal, Path });
alternative_walk!(misc::GenericArgument { Type, Binding });
alternative_walk!(misc::TraitApplication { Generic, Callable });
alternative_walk!(item::FunctionBody { Block, Declaration });
alternative_walk!(item::ModuleForm { External, Inline });

impl Fields for SyntaxSite {
    fn fields<'a>(&'a self, owner: &'a Arc<ParsedFile>, output: &mut dyn FnMut(ElementRef<'a>)) {
        FieldContext::new(owner, *self).remaining(0, output);
    }
}

impl Fields for misc::Name {
    fn fields<'a>(&'a self, owner: &'a Arc<ParsedFile>, output: &mut dyn FnMut(ElementRef<'a>)) {
        self.token.walk(
            FieldContext::new(owner, SyntaxSite::token(self.token.index())),
            output,
        );
    }
}

impl Fields for expr::Literal {
    fn fields<'a>(&'a self, owner: &'a Arc<ParsedFile>, output: &mut dyn FnMut(ElementRef<'a>)) {
        self.token.walk(
            FieldContext::new(owner, SyntaxSite::token(self.token.index())),
            output,
        );
    }
}

impl Fields for misc::Path {
    fn fields<'a>(&'a self, owner: &'a Arc<ParsedFile>, output: &mut dyn FnMut(ElementRef<'a>)) {
        match self {
            Self::Single(name) => name.walk(
                FieldContext::new(owner, SyntaxSite::token(name.token.index())),
                output,
            ),
            Self::Segmented(path) => path.fields(owner, output),
        }
    }
}

impl Fields for expr::PathExpr {
    fn fields<'a>(&'a self, owner: &'a Arc<ParsedFile>, output: &mut dyn FnMut(ElementRef<'a>)) {
        match self {
            Self::Plain(path) => {
                path.walk(FieldContext::new(owner, path.site()), output);
            }
            Self::Applied(path) => path.fields(owner, output),
            Self::Qualified(ty) => ty.walk(FieldContext::new(owner, ty.site), output),
        }
    }
}

impl Fields for ty::TypeSyntax {
    fn fields<'a>(&'a self, owner: &'a Arc<ParsedFile>, output: &mut dyn FnMut(ElementRef<'a>)) {
        let context = FieldContext::new(owner, self.site());
        match self {
            Self::Named(path) => path.walk(context, output),
            Self::Never(token) => token.walk(context, output),
            Self::Applied(ty) => ty.fields(owner, output),
            Self::Grouped(ty) => ty.fields(owner, output),
            Self::Tuple(ty) => ty.fields(owner, output),
            Self::Array(ty) => ty.fields(owner, output),
            Self::Function(ty) => ty.fields(owner, output),
            Self::Qualified(ty) => ty.fields(owner, output),
            Self::Error(site) => site.fields(owner, output),
        }
    }
}

impl Fields for expr::Pattern {
    fn fields<'a>(&'a self, owner: &'a Arc<ParsedFile>, output: &mut dyn FnMut(ElementRef<'a>)) {
        let context = FieldContext::new(owner, self.site());
        match self {
            Self::Wildcard(token) => token.walk(context, output),
            Self::Path(path) => path.walk(context, output),
            Self::Literal(literal) => literal.walk(context, output),
            Self::Or(pattern) => pattern.fields(owner, output),
            Self::Range(pattern) => pattern.fields(owner, output),
            Self::Grouped(pattern) => pattern.fields(owner, output),
            Self::Tuple(pattern) => pattern.fields(owner, output),
            Self::TupleStruct(pattern) => pattern.fields(owner, output),
            Self::Struct(pattern) => pattern.fields(owner, output),
            Self::Error(site) => site.fields(owner, output),
        }
    }
}

impl Fields for item::AttributeValue {
    fn fields<'a>(&'a self, owner: &'a Arc<ParsedFile>, output: &mut dyn FnMut(ElementRef<'a>)) {
        let context = FieldContext::new(owner, self.site());
        match self {
            Self::Literal(literal) => literal.walk(context, output),
            Self::Path(path) => path.walk(context, output),
            Self::List(list) => list.fields(owner, output),
            Self::Error(site) => site.fields(owner, output),
        }
    }
}

impl Walk for misc::BindingKeyword {
    fn walk<'a>(&'a self, context: FieldContext<'a>, output: &mut dyn FnMut(ElementRef<'a>)) {
        self.token.walk(context, output);
    }
}

impl Walk for item::ModuleId {
    fn walk<'a>(&'a self, context: FieldContext<'a>, output: &mut dyn FnMut(ElementRef<'a>)) {
        context.element(0, output);
        let body = &context.owner.modules[self.0 as usize];
        output(ElementRef::Node(NodeRef::ModuleBody(Ast {
            owner: context.owner,
            node: body,
        })));
    }
}

impl Walk for expr::ArrayContents {
    fn walk<'a>(&'a self, context: FieldContext<'a>, output: &mut dyn FnMut(ElementRef<'a>)) {
        match self {
            Self::List(elements) => elements.walk(context, output),
            Self::Repeat {
                value,
                separator,
                count,
            } => {
                context.element(0, output);
                value.walk(context.clear(), output);
                separator.walk(context.clear(), output);
                count.walk(context.clear(), output);
            }
        }
    }
}

impl Walk for item::Visibility {
    fn walk<'a>(&'a self, context: FieldContext<'a>, output: &mut dyn FnMut(ElementRef<'a>)) {
        context.element(0, output);
        if let Self::Public {
            keyword,
            restriction,
        } = self
        {
            keyword.walk(context.clear(), output);
            restriction.walk(context.clear(), output);
        }
    }
}

impl Walk for item::AttributePayload {
    fn walk<'a>(&'a self, context: FieldContext<'a>, output: &mut dyn FnMut(ElementRef<'a>)) {
        match self {
            Self::Arguments(arguments) => arguments.walk(context, output),
            Self::Value { equal, value } => {
                context.element(0, output);
                equal.walk(context.clear(), output);
                value.walk(context.clear(), output);
            }
        }
    }
}

impl Walk for item::UseSuffix {
    fn walk<'a>(&'a self, context: FieldContext<'a>, output: &mut dyn FnMut(ElementRef<'a>)) {
        context.element(0, output);
        match self {
            Self::Alias { keyword, name } => {
                keyword.walk(context.clear(), output);
                name.walk(context.clear(), output);
            }
            Self::Glob { separator, star } => {
                separator.walk(context.clear(), output);
                star.walk(context.clear(), output);
            }
            Self::Group { separator, group } => {
                separator.walk(context.clear(), output);
                group.walk(context.clear(), output);
            }
        }
    }
}

impl Walk for item::ConstInitializer {
    fn walk<'a>(&'a self, context: FieldContext<'a>, output: &mut dyn FnMut(ElementRef<'a>)) {
        context.element(0, output);
        if let Self::Definition { equal, value } = self {
            equal.walk(context.clear(), output);
            value.walk(context.clear(), output);
        }
    }
}

impl Walk for misc::ParameterType {
    fn walk<'a>(&'a self, context: FieldContext<'a>, output: &mut dyn FnMut(ElementRef<'a>)) {
        context.element(0, output);
        if let Self::Explicit { colon, ty } = self {
            colon.walk(context.clear(), output);
            ty.walk(context.clear(), output);
        }
    }
}

impl Walk for item::AliasTarget {
    fn walk<'a>(&'a self, context: FieldContext<'a>, output: &mut dyn FnMut(ElementRef<'a>)) {
        context.element(0, output);
        match self {
            Self::LeadingWhere { clause, equal, ty } => {
                clause.walk(context.clear(), output);
                equal.walk(context.clear(), output);
                ty.walk(context.clear(), output);
            }
            Self::TrailingWhere { equal, ty, clause } => {
                equal.walk(context.clear(), output);
                ty.walk(context.clear(), output);
                clause.walk(context.clear(), output);
            }
        }
    }
}

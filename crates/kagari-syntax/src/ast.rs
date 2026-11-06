//! Directly stored, lossless and recoverable grammar. Nodes are immutable after parsing.
//!
//! Owned node fields are declared once with `node!`; the same declaration derives
//! authored-order traversal and recovery field names. `Ast<T>` borrows those fields
//! with their physical source owner. `ModuleSyntax` is the independently retained
//! module handle; ordinary nodes do not own reference-counted handles.
//!
//! ```text
//! fn f() { if {} }
//! FnDef.body -> BlockExpr.tail -> Expr::IfExpr
//! IfExpr.condition = Missing { at: byte offset of `{` }
//! IfExpr.then_body = Present(BlockExpr { open, statements: [], tail: None, close })
//! IfExpr.else_clause = None
//! ```
//!
//! Missing roles consume no text. Real punctuation refers to the single lexical
//! table; unexpected contents retain an explicit owner/field/entry position.
//!
//! Start with [`borrow::Ast`] for source-aware access, [`site::Required`] and
//! [`site::SeparatedList`] for recovery states, [`module::ModuleSyntax`] for
//! retention and logical module traversal, and [`visit::NodeRef`] for physical
//! node traversal. A convenient `Option` getter or filtered list iterator can
//! omit recovery details; `Ast::data()` exposes the original named fields.
//!
//! # Reading the grammar models
//!
//! Concrete definitions in [`item`], [`expr`], [`stmt`], [`ty`] and [`misc`]
//! map source examples to their stored fields. Sketches abbreviate boxes,
//! `Required::Present` and list entries where noted; they are not Rust values
//! or complete debug dumps. Quoted punctuation denotes lexical tokens, while
//! named expressions, types and patterns denote child syntax. A `Name` or
//! `Literal` wraps a token reference rather than owning its spelling.
//!
//! `node!` adds a parse-local site to the declared fields; `group!` bundles
//! related fields without a separate node site. Neither owns source text.
//! Shared missing/optional-role semantics belong to [`site::Required`], and
//! comma retention and filtered entry positions to [`site::SeparatedList`].
//! Examples explain syntax shape, not successful name resolution or typing.

macro_rules! node {
    ($(#[$meta:meta])* $name:ident { $($field:ident: $ty:ty),* $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug)]
        pub struct $name {
            pub(crate) site: $crate::ast::site::SyntaxSite,
            $(pub $field: $ty,)*
        }

        impl $crate::ast::borrow::NodeData for $name {
            fn site(&self) -> $crate::ast::site::SyntaxSite {
                self.site
            }

            fn field_name(&self, field: u16) -> Option<&'static str> {
                const FIELDS: &[&str] = &[$(stringify!($field),)*];
                FIELDS.get(usize::from(field)).copied()
            }
        }

        impl $crate::ast::visit::Fields for $name {
            fn fields<'a>(
                &'a self,
                owner: &'a std::sync::Arc<$crate::parser::ParsedFile>,
                output: &mut dyn FnMut($crate::ast::visit::ElementRef<'a>),
            ) {
                let context = $crate::ast::visit::FieldContext::new(owner, self.site);
                let mut index = 0;
                $(
                    $crate::ast::visit::Walk::walk(&self.$field, context.field(index), output);
                    index += 1;
                )*
                context.remaining(index, output);
            }
        }
    };
}

macro_rules! group {
    ($(#[$meta:meta])* $name:ident { $($visibility:vis $field:ident: $ty:ty),* $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug)]
        pub struct $name { $($visibility $field: $ty,)* }

        impl $crate::ast::visit::Walk for $name {
            fn walk<'a>(&'a self, context: $crate::ast::visit::FieldContext<'a>, output: &mut dyn FnMut($crate::ast::visit::ElementRef<'a>)) {
                context.element(0, output);
                $($crate::ast::visit::Walk::walk(&self.$field, context.clear(), output);)*
            }
        }
    };
}

macro_rules! required_child {
    ($method:ident, $ty:ty, $($field:ident).+) => {
        pub fn $method(self) -> Option<$crate::ast::borrow::Ast<'a, $ty>> {
            self.node.$($field).+.as_ref().map(|node| self.with::<$ty>(node))
        }
    };
}

macro_rules! optional_child {
    ($method:ident, $ty:ty, $($field:ident).+) => {
        pub fn $method(self) -> Option<$crate::ast::borrow::Ast<'a, $ty>> {
            self.node.$($field).+.as_ref().map(|node| self.with::<$ty>(node))
        }
    };
}

macro_rules! required_expression {
    ($method:ident, $($field:ident).+) => {
        pub fn $method(self) -> Option<$crate::ast::views::ExprView<'a>> {
            self.node.$($field).+.as_ref().map(|node| self.with::<$crate::ast::expr::Expr>(node).view())
        }
    };
}

mod access;

pub mod borrow;
pub mod expr;
pub mod item;
pub mod misc;
pub mod module;
pub mod site;
pub mod stmt;
pub mod tokens;
pub mod ty;
pub mod views;
pub mod visit;

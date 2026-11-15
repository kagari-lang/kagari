//! Uninterpreted source attributes attached to lowering-local declaration identities.

use crate::hir::{
    ids::{FieldId, ImplId, TraitId, UseId, VariantId},
    item::Item,
};
use kagari_common::span::Span;

/// A trait/impl containing inline associated declarations.
///
/// IDs address the matching module's per-kind vectors, not persistent definitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AssociatedOwner {
    Trait(TraitId),
    Impl(ImplId),
}

/// An attribute's declaration/member in one lowering's owned HIR.
///
/// Methods and local functions use `Item::Function` with their canonical function ID.
/// Associated slots index the owner's `associated_consts` or `associated_types` list,
/// including required members without an initializer/type. Uses name an authored
/// declaration, not one of its flattened import leaves. Field/variant identities do
/// not imply that a consumer accepts them as policy targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AttributeTarget {
    Item(Item),
    Use(UseId),
    Field(FieldId),
    Variant(VariantId),
    AssociatedConst {
        owner: AssociatedOwner,
        index: usize,
    },
    AssociatedType {
        owner: AssociatedOwner,
        index: usize,
    },
}

/// A lowered attribute attached to a source item, without interpretation of its policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributeFact {
    /// Attribute name as written; recovery may produce an empty name.
    pub name: String,
    /// Exclusive authored form, including missing values and retained recovery.
    pub form: AttributeForm,
    /// Byte range of the entire attribute.
    pub span: Span,
    /// Declaration/member in the matching module, never a portable cross-lowering ID.
    pub target: AttributeTarget,
}

/// Exclusive attribute syntax; empty arguments differ from a bare attribute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttributeForm {
    Bare,
    Value(AttributeValue),
    Arguments(Vec<AttributeArgument>),
    /// Damaged outer syntax with at most one recognizable payload.
    Malformed {
        recovered: Option<AttributePayload>,
    },
}

/// Recognizable syntax retained inside a malformed attribute, without another wrapper.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttributePayload {
    Bare,
    Value(AttributeValue),
    Arguments(Vec<AttributeArgument>),
}

/// A positional or named value within an attribute argument/list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributeArgument {
    /// Argument key; absent for a positional argument.
    pub name: Option<String>,
    /// Literal, path, nested list, missing value or malformed retained syntax.
    pub value: AttributeValue,
}

/// Attribute syntax retained for consumers rather than evaluated during lowering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttributeValue {
    /// Literal source spelling, including its original delimiters/suffix.
    Literal(String),
    /// Unresolved source path spelling.
    Path(String),
    /// Nested positional/named elements in source order.
    List(Vec<AttributeArgument>),
    /// A required assigned/argument value absent in recovered syntax.
    Missing,
    /// A retained AST error or a value whose required structure is damaged.
    Malformed,
}

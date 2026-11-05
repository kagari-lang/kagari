//! Diagnostic type sizes for retained-storage attribution, outside timing windows.

use std::mem::size_of;

use kagari_syntax::ast::{
    expr::{BinaryExpr, BlockExpr, CallExpr, Expr, IfExpr},
    item::{FnDef, FunctionBody, Visibility},
    misc::{
        BoundClause, GenericParam, GenericParamList, Param, ParamList, ParameterType, Path,
        ReturnType, TraitApplication, TraitBoundList, TraitRef,
    },
    site::{ListEntry, Required, SyntaxSite},
    tokens::{Comma, Token},
    ty::TypeSyntax,
};
use serde_json::{Value, json};

pub fn report() -> Value {
    json!({
        "site": size_of::<SyntaxSite>(), "expr": size_of::<Expr>(),
        "binary": size_of::<BinaryExpr>(), "block": size_of::<BlockExpr>(),
        "call": size_of::<CallExpr>(), "if": size_of::<IfExpr>(),
        "fn": size_of::<FnDef>(), "body": size_of::<FunctionBody>(),
        "visibility": size_of::<Visibility>(), "path": size_of::<Path>(),
        "type": size_of::<TypeSyntax>(), "return": size_of::<ReturnType>(),
        "param_list": size_of::<ParamList>(), "param": size_of::<Param>(),
        "param_type": size_of::<ParameterType>(), "param_entry": size_of::<ListEntry<Param>>(),
        "generic_list": size_of::<GenericParamList>(), "generic": size_of::<GenericParam>(),
        "generic_entry": size_of::<ListEntry<GenericParam>>(), "bound_clause": size_of::<BoundClause>(),
        "trait_ref": size_of::<TraitRef>(), "trait_application": size_of::<TraitApplication>(),
        "bounds": size_of::<TraitBoundList>(), "required_token": size_of::<Required<Token<Comma>>>(),
        "optional_required_token": size_of::<Option<Required<Token<Comma>>>>(),
    })
}

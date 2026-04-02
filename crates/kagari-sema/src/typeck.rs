use kagari_common::Diagnostic;
use kagari_syntax::ast::{self, Item};

use crate::{resolver::NameTable, types::TypeId};

#[derive(Debug, Clone)]
pub struct TypedModule {
    pub functions: Vec<TypedFunction>,
}

#[derive(Debug, Clone)]
pub struct TypedFunction {
    pub name: String,
    pub params: Vec<TypedParameter>,
    pub return_type: TypeId,
}

#[derive(Debug, Clone)]
pub struct TypedParameter {
    pub name: String,
    pub ty: TypeId,
}

pub fn check_module(
    module: &ast::Module,
    _names: &NameTable,
) -> Result<TypedModule, Vec<Diagnostic>> {
    let mut diagnostics = Vec::new();
    let mut functions = Vec::new();

    for item in &module.items {
        let Item::Function(function) = item;

        let mut params = Vec::new();
        for param in &function.params {
            match TypeId::from_name(&param.ty.name) {
                Some(ty) => params.push(TypedParameter {
                    name: param.name.clone(),
                    ty,
                }),
                None => diagnostics.push(Diagnostic::error(format!(
                    "unknown parameter type `{}` in function `{}`",
                    param.ty.name, function.name
                ))),
            }
        }

        let return_type = match function.return_type.as_ref() {
            Some(ty) => match TypeId::from_name(&ty.name) {
                Some(ty) => ty,
                None => {
                    diagnostics.push(Diagnostic::error(format!(
                        "unknown return type `{}` in function `{}`",
                        ty.name, function.name
                    )));
                    TypeId::Builtin(crate::types::BuiltinType::Unit)
                }
            },
            None => TypeId::Builtin(crate::types::BuiltinType::Unit),
        };

        functions.push(TypedFunction {
            name: function.name.clone(),
            params,
            return_type,
        });
    }

    if diagnostics.is_empty() {
        Ok(TypedModule { functions })
    } else {
        Err(diagnostics)
    }
}

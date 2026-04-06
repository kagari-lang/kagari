use kagari_common::{Diagnostic, DiagnosticKind, TypePosition};
use kagari_syntax::ast::{self, AstNode, Item};
use smallvec::SmallVec;

use crate::{
    BoxedDiagnosticBuffer, FunctionBuffer, ParameterBuffer, resolver::NameTable, types::TypeId,
};

#[derive(Debug, Clone)]
pub struct TypedModule {
    pub functions: FunctionBuffer,
}

#[derive(Debug, Clone)]
pub struct TypedFunction {
    pub name: String,
    pub params: ParameterBuffer,
    pub return_type: TypeId,
}

#[derive(Debug, Clone)]
pub struct TypedParameter {
    pub name: String,
    pub ty: TypeId,
}

pub fn check_module(
    module: &ast::SourceFile,
    _names: &NameTable,
) -> Result<TypedModule, BoxedDiagnosticBuffer> {
    let mut diagnostics = SmallVec::<[Diagnostic; 4]>::new();
    let mut functions = SmallVec::new();

    for item in module.items() {
        let Item::FnDef(function) = item else {
            continue;
        };

        let mut params = SmallVec::new();
        let function_name = function
            .name_text()
            .unwrap_or_else(|| "<missing>".to_string());

        if let Some(param_list) = function.param_list() {
            for param in param_list.params() {
                let param_name = param.name_text().unwrap_or_else(|| "<missing>".to_string());
                let param_ty_name = param
                    .ty()
                    .and_then(|ty| ty.name_text())
                    .unwrap_or_else(|| "<missing>".to_string());

                match TypeId::from_name(&param_ty_name) {
                    Some(ty) => params.push(TypedParameter {
                        name: param_name,
                        ty,
                    }),
                    None => diagnostics.push(
                        Diagnostic::error(DiagnosticKind::UnknownType {
                            type_name: param_ty_name,
                            function_name: function_name.clone(),
                            position: TypePosition::Parameter,
                        })
                        .with_span(syntax_span(&param)),
                    ),
                }
            }
        }

        let return_type = match function.return_type().and_then(|ty| ty.name_text()) {
            Some(ty_name) => match TypeId::from_name(&ty_name) {
                Some(ty) => ty,
                None => {
                    diagnostics.push(
                        Diagnostic::error(DiagnosticKind::UnknownType {
                            type_name: ty_name,
                            function_name: function_name.clone(),
                            position: TypePosition::Return,
                        })
                        .with_span(
                            function
                                .return_type()
                                .map(|ty| syntax_span(&ty))
                                .unwrap_or_else(|| syntax_span(&function)),
                        ),
                    );
                    TypeId::Builtin(crate::types::BuiltinType::Unit)
                }
            },
            None => TypeId::Builtin(crate::types::BuiltinType::Unit),
        };

        functions.push(TypedFunction {
            name: function_name,
            params,
            return_type,
        });
    }

    if diagnostics.is_empty() {
        Ok(TypedModule { functions })
    } else {
        Err(Box::new(diagnostics))
    }
}

fn syntax_span(node: &impl AstNode) -> kagari_common::Span {
    let range = node.syntax().text_range();
    kagari_common::Span::new(range.start().into(), range.end().into())
}

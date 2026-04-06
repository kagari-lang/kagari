use kagari_common::{Diagnostic, DiagnosticKind, TypePosition};
use smallvec::SmallVec;

use crate::{
    BoxedDiagnosticBuffer,
    lower::LoweredModule,
    resolver::ResolvedNames,
    typeck::body::BodyChecker,
    typeck::ty::{display_type, display_type_id, resolve_type},
    typeck::{
        BodyTypeEnv, FunctionTypeIndex, TypeTable, TypedFunction, TypedModule, TypedParameter,
    },
    types::{BuiltinType, TypeId},
};

pub fn check_module(
    lowered: &LoweredModule,
    names: &ResolvedNames,
) -> Result<TypedModule, BoxedDiagnosticBuffer> {
    let mut diagnostics = SmallVec::<[Diagnostic; 4]>::new();
    let mut functions = SmallVec::new();
    let mut function_index = FunctionTypeIndex::default();

    for function in &lowered.module.functions {
        let mut params = SmallVec::new();
        let function_name = if function.name.is_empty() {
            "<missing>".to_string()
        } else {
            function.name.clone()
        };

        for param in &function.params {
            let param_name = if param.name.is_empty() {
                "<missing>".to_string()
            } else {
                param.name.clone()
            };
            let param_ty_name = display_type(&lowered.module, param.ty);

            match resolve_type(&lowered.module, param.ty) {
                Some(ty) => params.push(TypedParameter {
                    id: param.id,
                    name: param_name,
                    ty,
                }),
                None => diagnostics.push(
                    Diagnostic::error(DiagnosticKind::UnknownType {
                        type_name: param_ty_name,
                        function_name: function_name.clone(),
                        position: TypePosition::Parameter,
                    })
                    .with_span(lowered.source_map.type_span(param.ty)),
                ),
            }
        }

        let return_type = match &function.return_type {
            Some(ty) => match resolve_type(&lowered.module, *ty) {
                Some(ty) => ty,
                None => {
                    let ty_name = display_type(&lowered.module, *ty);
                    diagnostics.push(
                        Diagnostic::error(DiagnosticKind::UnknownType {
                            type_name: ty_name,
                            function_name: function_name.clone(),
                            position: TypePosition::Return,
                        })
                        .with_span(lowered.source_map.type_span(*ty)),
                    );
                    TypeId::Builtin(BuiltinType::Unit)
                }
            },
            None => TypeId::Builtin(BuiltinType::Unit),
        };

        let typed_function = TypedFunction {
            name: function_name,
            params,
            return_type,
        };
        function_index
            .by_id
            .insert(function.id, typed_function.clone());
        functions.push(typed_function);
    }

    if !diagnostics.is_empty() {
        Err(Box::new(diagnostics))
    } else {
        let mut type_table = TypeTable::default();
        for function in &lowered.module.functions {
            let mut env = BodyTypeEnv::default();
            if let Some(typed_function) = function_index.by_id.get(&function.id) {
                for param in &typed_function.params {
                    env.params.insert(param.id, param.ty);
                }
                let mut checker = BodyChecker::new(
                    lowered,
                    names,
                    &function_index,
                    &mut diagnostics,
                    &mut type_table,
                    &typed_function.name,
                    typed_function.return_type,
                );
                let body_ty = checker.infer_block_types(function.body, &mut env);
                if body_ty != typed_function.return_type {
                    diagnostics.push(
                        Diagnostic::error(DiagnosticKind::ReturnTypeMismatch {
                            function_name: typed_function.name.clone(),
                            expected: display_type_id(typed_function.return_type).to_string(),
                            found: display_type_id(body_ty).to_string(),
                        })
                        .with_span(lowered.source_map.function_span(function.id)),
                    );
                }
            }
        }

        if diagnostics.is_empty() {
            Ok(TypedModule {
                functions,
                type_table,
            })
        } else {
            Err(Box::new(diagnostics))
        }
    }
}

mod expr;
mod function;
mod state;
mod stmt;
mod support;

use std::collections::HashMap;

use kagari_hir::AnalyzedModule;
use kagari_hir::hir::{
    BinaryOp, ConstId, ExprId, ExprKind, FunctionId, LiteralKind, LocalId, PlaceId, PrefixOp,
};
use kagari_hir::resolver::ResolvedName;

use crate::module::{Constant, IrModule, IrModuleSlot, ModuleSlotId, ValueType};

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum EvaluatedConst {
    Scalar(Constant),
    Tuple(Vec<EvaluatedConst>),
    Array(Vec<EvaluatedConst>),
    Struct {
        name: String,
        fields: Vec<EvaluatedConstField>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct EvaluatedConstField {
    pub(crate) name: String,
    pub(crate) value: EvaluatedConst,
}

#[derive(Debug)]
pub enum IrLoweringError {
    MissingTypedFunction(FunctionId),
    MissingExprType(ExprId),
    MissingLocalType(LocalId),
    UnresolvedExpr(ExprId),
    UnresolvedPlace(PlaceId),
    MissingBinding(&'static str),
    UnsupportedExpr(&'static str),
    UnsupportedStatement(&'static str),
    UnsupportedConstExpr(&'static str),
    ConstCycle(ConstId),
    InvalidLoopControl,
}

pub fn lower_to_ir(module: &AnalyzedModule) -> Result<IrModule, IrLoweringError> {
    let const_values = lower_const_values(module)?;
    let mut static_slots = HashMap::new();
    let mut module_slots = Vec::new();

    for static_item in &module.lowered.module.statics {
        let ty = module
            .typed
            .statics
            .get(&static_item.id)
            .map(|item| ValueType::from_type_id(&item.ty))
            .unwrap_or(ValueType::Unit);
        let id = ModuleSlotId::new(module_slots.len());
        static_slots.insert(static_item.id, id);
        module_slots.push(IrModuleSlot {
            id,
            name: static_item.name.clone(),
            ty,
            mutable: static_item.mutable,
        });
    }

    let functions = module
        .lowered
        .module
        .functions
        .iter()
        .map(|function| function::lower_function(module, function, &const_values, &static_slots))
        .collect::<Result<Vec<_>, _>>()?;

    Ok(IrModule {
        module_init: module.lowered.module.module_init,
        module_slots,
        functions,
    })
}

fn lower_const_values(
    module: &AnalyzedModule,
) -> Result<HashMap<ConstId, EvaluatedConst>, IrLoweringError> {
    fn eval_const(
        module: &AnalyzedModule,
        const_id: ConstId,
        cache: &mut HashMap<ConstId, EvaluatedConst>,
        visiting: &mut Vec<ConstId>,
    ) -> Result<EvaluatedConst, IrLoweringError> {
        if let Some(value) = cache.get(&const_id) {
            return Ok(value.clone());
        }
        if visiting.contains(&const_id) {
            return Err(IrLoweringError::ConstCycle(const_id));
        }
        visiting.push(const_id);

        let const_item = module
            .lowered
            .module
            .consts
            .iter()
            .find(|item| item.id == const_id)
            .ok_or(IrLoweringError::MissingBinding("const item"))?;
        let value = eval_const_expr(module, const_item.initializer, cache, visiting)?;
        visiting.pop();
        cache.insert(const_id, value.clone());
        Ok(value)
    }

    fn eval_const_expr(
        module: &AnalyzedModule,
        expr_id: ExprId,
        cache: &mut HashMap<ConstId, EvaluatedConst>,
        visiting: &mut Vec<ConstId>,
    ) -> Result<EvaluatedConst, IrLoweringError> {
        let expr = module.lowered.module.expr(expr_id);
        match &expr.kind {
            ExprKind::Literal(literal) => match literal.kind {
                LiteralKind::Number => Ok(EvaluatedConst::Scalar(Constant::I32(
                    literal.text.parse::<i32>().unwrap_or_default(),
                ))),
                LiteralKind::Float => Ok(EvaluatedConst::Scalar(Constant::F32(
                    literal.text.parse::<f32>().unwrap_or_default(),
                ))),
                LiteralKind::String => {
                    Ok(EvaluatedConst::Scalar(Constant::Str(literal.text.clone())))
                }
                LiteralKind::Bool => Ok(EvaluatedConst::Scalar(Constant::Bool(
                    literal.text == "true",
                ))),
            },
            ExprKind::Name(_) => {
                let resolved = module
                    .names
                    .expr_resolution(expr_id)
                    .ok_or(IrLoweringError::UnresolvedExpr(expr_id))?;
                match resolved {
                    ResolvedName::Const(id) => eval_const(module, id, cache, visiting),
                    _ => Err(IrLoweringError::UnsupportedConstExpr(
                        "const initializer must reference other consts or literals only",
                    )),
                }
            }
            ExprKind::Prefix { op, expr } => {
                let value = eval_const_expr(module, *expr, cache, visiting)?;
                match (op, value) {
                    (PrefixOp::Neg, EvaluatedConst::Scalar(Constant::I32(value))) => {
                        Ok(EvaluatedConst::Scalar(Constant::I32(-value)))
                    }
                    (PrefixOp::Neg, EvaluatedConst::Scalar(Constant::F32(value))) => {
                        Ok(EvaluatedConst::Scalar(Constant::F32(-value)))
                    }
                    (PrefixOp::Not, EvaluatedConst::Scalar(Constant::Bool(value))) => {
                        Ok(EvaluatedConst::Scalar(Constant::Bool(!value)))
                    }
                    _ => Err(IrLoweringError::UnsupportedConstExpr(
                        "unsupported unary const expression",
                    )),
                }
            }
            ExprKind::Binary { lhs, op, rhs } => {
                let lhs = eval_const_expr(module, *lhs, cache, visiting)?;
                let rhs = eval_const_expr(module, *rhs, cache, visiting)?;
                eval_const_binary(op, lhs, rhs)
            }
            ExprKind::Tuple(elements) => Ok(EvaluatedConst::Tuple(
                elements
                    .iter()
                    .map(|expr| eval_const_expr(module, *expr, cache, visiting))
                    .collect::<Result<Vec<_>, _>>()?,
            )),
            ExprKind::Array(elements) => Ok(EvaluatedConst::Array(
                elements
                    .iter()
                    .map(|expr| eval_const_expr(module, *expr, cache, visiting))
                    .collect::<Result<Vec<_>, _>>()?,
            )),
            ExprKind::StructInit { path, fields } => {
                let fields = fields
                    .iter()
                    .map(|field| {
                        Ok(EvaluatedConstField {
                            name: field.name.clone(),
                            value: eval_const_expr(module, field.value, cache, visiting)?,
                        })
                    })
                    .collect::<Result<Vec<_>, IrLoweringError>>()?;
                Ok(EvaluatedConst::Struct {
                    name: path.clone(),
                    fields,
                })
            }
            _ => Err(IrLoweringError::UnsupportedConstExpr(
                "unsupported const initializer expression",
            )),
        }
    }

    fn eval_const_binary(
        op: &BinaryOp,
        lhs: EvaluatedConst,
        rhs: EvaluatedConst,
    ) -> Result<EvaluatedConst, IrLoweringError> {
        match (op, lhs, rhs) {
            (
                BinaryOp::Add,
                EvaluatedConst::Scalar(Constant::I32(lhs)),
                EvaluatedConst::Scalar(Constant::I32(rhs)),
            ) => Ok(EvaluatedConst::Scalar(Constant::I32(lhs + rhs))),
            (
                BinaryOp::Sub,
                EvaluatedConst::Scalar(Constant::I32(lhs)),
                EvaluatedConst::Scalar(Constant::I32(rhs)),
            ) => Ok(EvaluatedConst::Scalar(Constant::I32(lhs - rhs))),
            (
                BinaryOp::Mul,
                EvaluatedConst::Scalar(Constant::I32(lhs)),
                EvaluatedConst::Scalar(Constant::I32(rhs)),
            ) => Ok(EvaluatedConst::Scalar(Constant::I32(lhs * rhs))),
            (
                BinaryOp::Div,
                EvaluatedConst::Scalar(Constant::I32(lhs)),
                EvaluatedConst::Scalar(Constant::I32(rhs)),
            ) => Ok(EvaluatedConst::Scalar(Constant::I32(lhs / rhs))),
            (
                BinaryOp::Add,
                EvaluatedConst::Scalar(Constant::F32(lhs)),
                EvaluatedConst::Scalar(Constant::F32(rhs)),
            ) => Ok(EvaluatedConst::Scalar(Constant::F32(lhs + rhs))),
            (
                BinaryOp::Sub,
                EvaluatedConst::Scalar(Constant::F32(lhs)),
                EvaluatedConst::Scalar(Constant::F32(rhs)),
            ) => Ok(EvaluatedConst::Scalar(Constant::F32(lhs - rhs))),
            (
                BinaryOp::Mul,
                EvaluatedConst::Scalar(Constant::F32(lhs)),
                EvaluatedConst::Scalar(Constant::F32(rhs)),
            ) => Ok(EvaluatedConst::Scalar(Constant::F32(lhs * rhs))),
            (
                BinaryOp::Div,
                EvaluatedConst::Scalar(Constant::F32(lhs)),
                EvaluatedConst::Scalar(Constant::F32(rhs)),
            ) => Ok(EvaluatedConst::Scalar(Constant::F32(lhs / rhs))),
            (BinaryOp::Eq, lhs, rhs) => Ok(EvaluatedConst::Scalar(Constant::Bool(lhs == rhs))),
            (BinaryOp::NotEq, lhs, rhs) => Ok(EvaluatedConst::Scalar(Constant::Bool(lhs != rhs))),
            (
                BinaryOp::Lt,
                EvaluatedConst::Scalar(Constant::I32(lhs)),
                EvaluatedConst::Scalar(Constant::I32(rhs)),
            ) => Ok(EvaluatedConst::Scalar(Constant::Bool(lhs < rhs))),
            (
                BinaryOp::Gt,
                EvaluatedConst::Scalar(Constant::I32(lhs)),
                EvaluatedConst::Scalar(Constant::I32(rhs)),
            ) => Ok(EvaluatedConst::Scalar(Constant::Bool(lhs > rhs))),
            (
                BinaryOp::Le,
                EvaluatedConst::Scalar(Constant::I32(lhs)),
                EvaluatedConst::Scalar(Constant::I32(rhs)),
            ) => Ok(EvaluatedConst::Scalar(Constant::Bool(lhs <= rhs))),
            (
                BinaryOp::Ge,
                EvaluatedConst::Scalar(Constant::I32(lhs)),
                EvaluatedConst::Scalar(Constant::I32(rhs)),
            ) => Ok(EvaluatedConst::Scalar(Constant::Bool(lhs >= rhs))),
            (
                BinaryOp::Lt,
                EvaluatedConst::Scalar(Constant::F32(lhs)),
                EvaluatedConst::Scalar(Constant::F32(rhs)),
            ) => Ok(EvaluatedConst::Scalar(Constant::Bool(lhs < rhs))),
            (
                BinaryOp::Gt,
                EvaluatedConst::Scalar(Constant::F32(lhs)),
                EvaluatedConst::Scalar(Constant::F32(rhs)),
            ) => Ok(EvaluatedConst::Scalar(Constant::Bool(lhs > rhs))),
            (
                BinaryOp::Le,
                EvaluatedConst::Scalar(Constant::F32(lhs)),
                EvaluatedConst::Scalar(Constant::F32(rhs)),
            ) => Ok(EvaluatedConst::Scalar(Constant::Bool(lhs <= rhs))),
            (
                BinaryOp::Ge,
                EvaluatedConst::Scalar(Constant::F32(lhs)),
                EvaluatedConst::Scalar(Constant::F32(rhs)),
            ) => Ok(EvaluatedConst::Scalar(Constant::Bool(lhs >= rhs))),
            (
                BinaryOp::AndAnd,
                EvaluatedConst::Scalar(Constant::Bool(lhs)),
                EvaluatedConst::Scalar(Constant::Bool(rhs)),
            ) => Ok(EvaluatedConst::Scalar(Constant::Bool(lhs && rhs))),
            (
                BinaryOp::OrOr,
                EvaluatedConst::Scalar(Constant::Bool(lhs)),
                EvaluatedConst::Scalar(Constant::Bool(rhs)),
            ) => Ok(EvaluatedConst::Scalar(Constant::Bool(lhs || rhs))),
            _ => Err(IrLoweringError::UnsupportedConstExpr(
                "unsupported binary const expression",
            )),
        }
    }

    let mut cache = HashMap::new();
    let mut visiting = Vec::new();
    for const_item in &module.lowered.module.consts {
        let value = eval_const(module, const_item.id, &mut cache, &mut visiting)?;
        cache.insert(const_item.id, value);
    }
    Ok(cache)
}

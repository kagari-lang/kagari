//! Shared bounded shape rules for type predicates and canonical encoding.
use crate::ty::{
    NominalTy, Ty,
    substitution::{MAX_TYPE_DEPTH, MAX_TYPE_NODES, TypeTransformError},
};
use kagari_common::cancellation::CancellationToken;

pub(super) struct ShapeBudget {
    visited: usize,
}

impl ShapeBudget {
    pub(super) fn new() -> Self {
        Self { visited: 0 }
    }

    pub(super) fn enter(&mut self, ty: &Ty, depth: usize) -> Result<(), TypeTransformError> {
        if depth > MAX_TYPE_DEPTH || self.visited >= MAX_TYPE_NODES {
            return Err(TypeTransformError::LimitExceeded);
        }
        let children = match ty {
            Ty::Host(_) | Ty::SelfType(_) | Ty::Parameter { .. } | Ty::Builtin(_) => 0,
            Ty::Reference { .. } | Ty::Range(..) | Ty::Iter(_) | Ty::Array(_) => 1,

            Ty::Tuple(elements) => elements.len(),
            Ty::Function { params, .. } => params.len().saturating_add(1),
            Ty::Struct(nominal) | Ty::NativeObject(nominal) | Ty::Enum(nominal) => {
                if !nominal.associated_types.is_empty() {
                    return Err(TypeTransformError::InvalidContract);
                }
                nominal.arguments.len()
            }
            Ty::Trait(nominal) => nominal_children(nominal),
            Ty::Projection {
                interface,
                arguments,
                ..
            } => nominal_children(interface)
                .saturating_add(arguments.len())
                .saturating_add(1),
        };
        self.enter_children(children, depth)
    }

    fn enter_children(&mut self, children: usize, depth: usize) -> Result<(), TypeTransformError> {
        if depth > MAX_TYPE_DEPTH || self.visited >= MAX_TYPE_NODES {
            return Err(TypeTransformError::LimitExceeded);
        }
        self.visited += 1;
        if children > MAX_TYPE_NODES - self.visited {
            return Err(TypeTransformError::LimitExceeded);
        }
        Ok(())
    }

    fn visit(
        &mut self,
        ty: &Ty,
        depth: usize,
        cancel: Option<&CancellationToken>,
    ) -> Result<(), TypeTransformError> {
        check_cancel(cancel)?;
        self.enter(ty, depth)?;
        // enter rejects depth > 64 before descending. No child vectors or wire
        // records are constructed, and the Rust call stack is bounded by depth.
        match ty {
            Ty::Host(_) | Ty::SelfType(_) | Ty::Parameter { .. } | Ty::Builtin(_) => {}
            Ty::Reference { target, .. }
            | Ty::Range(target, _)
            | Ty::Iter(target)
            | Ty::Array(target) => self.visit(target, depth + 1, cancel)?,

            Ty::Tuple(elements) => {
                for element in elements {
                    self.visit(element, depth + 1, cancel)?;
                }
            }
            Ty::Function { params, result } => {
                for parameter in params {
                    self.visit(parameter, depth + 1, cancel)?;
                }
                self.visit(result, depth + 1, cancel)?;
            }
            Ty::Struct(nominal)
            | Ty::NativeObject(nominal)
            | Ty::Enum(nominal)
            | Ty::Trait(nominal) => self.nominal_children(nominal, depth + 1, cancel)?,
            Ty::Projection {
                receiver,
                interface,
                arguments,
                ..
            } => {
                self.visit(receiver, depth + 1, cancel)?;
                self.nominal_children(interface, depth + 1, cancel)?;
                for argument in arguments {
                    self.visit(argument, depth + 1, cancel)?;
                }
            }
        }
        Ok(())
    }

    fn nominal_children(
        &mut self,
        nominal: &NominalTy,
        depth: usize,
        cancel: Option<&CancellationToken>,
    ) -> Result<(), TypeTransformError> {
        for argument in &nominal.arguments {
            self.visit(argument, depth, cancel)?;
        }
        for value in nominal.associated_types.values() {
            self.visit(value, depth, cancel)?;
        }
        Ok(())
    }
}

fn nominal_children(nominal: &NominalTy) -> usize {
    nominal
        .arguments
        .len()
        .saturating_add(nominal.associated_types.len())
}

fn check_cancel(cancel: Option<&CancellationToken>) -> Result<(), TypeTransformError> {
    if let Some(cancel) = cancel {
        cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
    }
    Ok(())
}

pub(super) fn validate(
    ty: &Ty,
    cancel: Option<&CancellationToken>,
) -> Result<(), TypeTransformError> {
    ShapeBudget::new().visit(ty, 1, cancel)
}

/// Validate a nominal as a complete trait-type tree, without cloning its fields.
pub(super) fn validate_nominal(
    nominal: &NominalTy,
    cancel: &CancellationToken,
) -> Result<(), TypeTransformError> {
    check_cancel(Some(cancel))?;
    let mut budget = ShapeBudget::new();
    budget.enter_children(nominal_children(nominal), 1)?;
    budget.nominal_children(nominal, 2, Some(cancel))
}

//! Preflight existing leaf codecs before they allocate their bounded wire trees.
use crate::{
    callable::{
        construction::evidence::{ConstructionSelection, ConstructionWitness},
        evidence::EvidenceRef,
        generic::GenericBody,
        interface::InterfaceCallContract,
        shared::SharedCall,
        witness::OperationWitness,
    },
    native_import::NativeImport,
    types::ConcreteFunctionIdentity,
    updates::{
        admission::nested,
        limits::{Budget, UpdateError},
    },
};
use kagari_types::{
    callable::Signature,
    declaration::requirement::NativeCallableRequirement,
    ty::{GenericBound, NominalTy, Ty},
};

pub(super) trait LeafShape {
    fn check_leaf(&self, budget: &mut Budget<'_>) -> Result<(), UpdateError>;
}

impl LeafShape for Ty {
    fn check_leaf(&self, budget: &mut Budget<'_>) -> Result<(), UpdateError> {
        if !self.within_wire_limits() {
            return Err(UpdateError::LimitExceeded);
        }
        type_nodes(self, 1, &mut 8192, budget)
    }
}

fn type_nodes(
    ty: &Ty,
    depth: usize,
    left: &mut usize,
    budget: &mut Budget<'_>,
) -> Result<(), UpdateError> {
    if depth > 64 {
        return Err(UpdateError::LimitExceeded);
    }
    *left = left.checked_sub(1).ok_or(UpdateError::LimitExceeded)?;
    budget.records(1)?;
    match ty {
        Ty::Host(_) | Ty::SelfType(_) | Ty::Parameter { .. } | Ty::Builtin(_) => {}
        Ty::Reference { target, .. }
        | Ty::Range(target, _)
        | Ty::Iter(target)
        | Ty::Array(target)
        | Ty::Set(target, _) => type_nodes(target, depth + 1, left, budget)?,
        Ty::Map { key, value, .. } => {
            type_nodes(key, depth + 1, left, budget)?;
            type_nodes(value, depth + 1, left, budget)?;
        }
        Ty::Tuple(elements) => {
            for ty in elements {
                type_nodes(ty, depth + 1, left, budget)?;
            }
        }
        Ty::Function { params, result } => {
            for ty in params {
                type_nodes(ty, depth + 1, left, budget)?;
            }
            type_nodes(result, depth + 1, left, budget)?;
        }
        Ty::Struct(nominal)
        | Ty::NativeObject(nominal)
        | Ty::Enum(nominal)
        | Ty::Trait(nominal) => nominal_nodes(nominal, depth + 1, left, budget)?,
        Ty::Projection {
            receiver,
            interface,
            arguments,
            ..
        } => {
            type_nodes(receiver, depth + 1, left, budget)?;
            nominal_nodes(interface, depth + 1, left, budget)?;
            for ty in arguments {
                type_nodes(ty, depth + 1, left, budget)?;
            }
        }
    }
    Ok(())
}

fn nominal_nodes(
    nominal: &NominalTy,
    depth: usize,
    left: &mut usize,
    budget: &mut Budget<'_>,
) -> Result<(), UpdateError> {
    nested(nominal.arguments.len())?;
    nested(nominal.associated_types.len())?;
    for ty in nominal
        .arguments
        .iter()
        .chain(nominal.associated_types.values())
    {
        type_nodes(ty, depth, left, budget)?;
    }
    Ok(())
}

fn nominal(nominal: &NominalTy, budget: &mut Budget<'_>) -> Result<(), UpdateError> {
    nominal_nodes(nominal, 2, &mut 8191, budget)
}

fn types(types: &[Ty], budget: &mut Budget<'_>) -> Result<(), UpdateError> {
    nested(types.len())?;
    for ty in types {
        ty.check_leaf(budget)?;
    }
    Ok(())
}

impl LeafShape for Signature {
    fn check_leaf(&self, budget: &mut Budget<'_>) -> Result<(), UpdateError> {
        types(&self.params, budget)?;
        self.result.check_leaf(budget)
    }
}

impl LeafShape for ConcreteFunctionIdentity {
    fn check_leaf(&self, budget: &mut Budget<'_>) -> Result<(), UpdateError> {
        types(&self.arguments, budget)
    }
}

fn bounds(bounds: &[GenericBound], budget: &mut Budget<'_>) -> Result<(), UpdateError> {
    nested(bounds.len())?;
    budget.records(bounds.len())?;
    for bound in bounds {
        bound.ty.check_leaf(budget)?;
        nested(bound.constraints.len())?;
        for ty in &bound.constraints {
            nominal(ty, budget)?;
        }
    }
    Ok(())
}

impl LeafShape for GenericBody {
    fn check_leaf(&self, budget: &mut Budget<'_>) -> Result<(), UpdateError> {
        nested(self.parameters.len())?;
        budget.records(self.parameters.len())?;
        bounds(&self.bounds, budget)
    }
}

fn shared_head(call: &SharedCall, budget: &mut Budget<'_>) -> Result<(), UpdateError> {
    call.instance.check_leaf(budget)?;
    call.signature.check_leaf(budget)?;
    types(&call.arguments, budget)
}

impl LeafShape for SharedCall {
    fn check_leaf(&self, budget: &mut Budget<'_>) -> Result<(), UpdateError> {
        shared_head(self, budget)?;
        witnesses(&self.operations, &self.constructions, budget)
    }
}

impl LeafShape for InterfaceCallContract {
    fn check_leaf(&self, budget: &mut Budget<'_>) -> Result<(), UpdateError> {
        nominal(&self.interface, budget)?;
        types(&self.arguments, budget)?;
        if let Some(receiver) = &self.receiver {
            receiver.check_leaf(budget)?;
        }
        nested(self.normalizations.len())?;
        for fact in &self.normalizations {
            fact.source.check_leaf(budget)?;
            fact.result.check_leaf(budget)?;
        }
        witnesses(&self.operations, &self.constructions, budget)
    }
}

impl LeafShape for NativeImport {
    fn check_leaf(&self, budget: &mut Budget<'_>) -> Result<(), UpdateError> {
        self.instance.check_leaf(budget)?;
        self.signature.check_leaf(budget)?;
        if let Some(generic) = &self.generic {
            generic.check_leaf(budget)?;
        }
        if let Some(adapter) = &self.result_adapter {
            adapter.receiver.check_leaf(budget)?;
            adapter.implementation.check_leaf(budget)?;
        }
        bounds(&self.requirements, budget)?;
        witnesses(&self.callables, &[], budget)
    }
}

impl LeafShape for OperationWitness {
    fn check_leaf(&self, budget: &mut Budget<'_>) -> Result<(), UpdateError> {
        evidence(EvidenceRef::Operation(self), budget)
    }
}

impl LeafShape for ConstructionWitness {
    fn check_leaf(&self, budget: &mut Budget<'_>) -> Result<(), UpdateError> {
        evidence(EvidenceRef::Construction(self), budget)
    }
}

fn requirement(
    value: &NativeCallableRequirement,
    budget: &mut Budget<'_>,
) -> Result<(), UpdateError> {
    value.receiver.check_leaf(budget)?;
    nominal(&value.interface, budget)?;
    types(&value.arguments, budget)
}

fn witnesses(
    operations: &[OperationWitness],
    constructions: &[ConstructionWitness],
    budget: &mut Budget<'_>,
) -> Result<(), UpdateError> {
    nested(operations.len())?;
    nested(constructions.len())?;
    for operation in operations {
        operation.check_leaf(budget)?;
    }
    for construction in constructions {
        construction.check_leaf(budget)?;
    }
    Ok(())
}

fn evidence(root: EvidenceRef<'_>, budget: &mut Budget<'_>) -> Result<(), UpdateError> {
    root.check_shape(budget.cancel)?;
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        budget.records(1)?;
        match node {
            EvidenceRef::Operation(OperationWitness::Selected(call)) => {
                requirement(&call.requirement, budget)?;
                call.instance.check_leaf(budget)?;
                call.signature.check_leaf(budget)?;
            }
            EvidenceRef::Operation(OperationWitness::Forward(value)) => requirement(value, budget)?,
            EvidenceRef::Operation(OperationWitness::SharedMethod(method)) => {
                requirement(&method.requirement, budget)?;
                method.implementation.check_leaf(budget)?;
            }
            EvidenceRef::Operation(OperationWitness::SharedAdapter(adapter)) => {
                requirement(&adapter.requirement, budget)?;
                shared_head(&adapter.call, budget)?;
            }
            EvidenceRef::Construction(witness) => {
                witness.requirement.receiver.check_leaf(budget)?;
                nominal(&witness.requirement.interface, budget)?;
                if let ConstructionSelection::Declared { implementation, .. } = &witness.selection {
                    implementation.check_leaf(budget)?;
                }
            }
        }
        let (operations, constructions) = node.children();
        budget.visit()?;
        pending.extend(operations.iter().map(EvidenceRef::Operation));
        pending.extend(constructions.iter().map(EvidenceRef::Construction));
    }
    Ok(())
}

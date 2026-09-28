use crate::{
    scalar::BuiltinType,
    standard::{
        intrinsic,
        surface::{StandardEnum, StandardTypeConstraint},
        traits::StandardTrait,
    },
    types::{
        AbiType, ConstraintAbi, GenericBoundAbi,
        proofs::{Budget, ProofCatalog, search::Search},
        substitution::TypeTransformError,
    },
};

impl ProofCatalog<'_> {
    pub(super) fn structural(
        &self,
        kind: StandardTrait,
        receiver: &AbiType,
        assumptions: &[GenericBoundAbi],
        search: &mut Search,
        budget: &Budget<'_>,
        depth: usize,
    ) -> Result<bool, TypeTransformError> {
        budget.step(depth)?;
        if matches!(kind, StandardTrait::PartialOrd | StandardTrait::Ord) {
            return Ok(match receiver {
                AbiType::Builtin(BuiltinType::F32 | BuiltinType::F64) => {
                    kind == StandardTrait::PartialOrd
                }
                AbiType::Builtin(_)
                | AbiType::StandardEnum {
                    kind: StandardEnum::Ordering,
                    ..
                } => true,
                _ => false,
            });
        }
        if !kind.equality_protocol()
            && !matches!(kind, StandardTrait::Debug | StandardTrait::Display)
        {
            return Ok(false);
        }
        if kind == StandardTrait::PartialEq
            && assumptions
                .iter()
                .filter(|bound| bound.ty == *receiver)
                .flat_map(|bound| &bound.constraints)
                .any(|bound| {
                    *bound == ConstraintAbi::Trait(intrinsic::applied(StandardTrait::Eq, vec![]))
                })
        {
            return Ok(true);
        }
        if matches!(receiver, AbiType::Struct(_) | AbiType::Enum(_))
            && matches!(kind, StandardTrait::Eq | StandardTrait::Hash)
            && self.explicit(
                &intrinsic::applied(StandardTrait::PartialEq, vec![]),
                receiver,
                assumptions,
                search,
                budget,
                depth,
            )? != 0
        {
            // Once nominal equality is customized, identity/structural Eq and
            // Hash defaults cannot silently disagree with that implementation.
            return Ok(false);
        }
        let members: Vec<&AbiType> = match receiver {
            AbiType::Builtin(ty) => {
                return Ok(!matches!(kind, StandardTrait::Eq | StandardTrait::Hash)
                    || !matches!(ty, BuiltinType::F32 | BuiltinType::F64));
            }
            _ if kind == StandardTrait::Display => return Ok(false),
            AbiType::Host(_) => return Ok(kind == StandardTrait::Debug),
            AbiType::Enum(_) if kind == StandardTrait::Debug => return Ok(true),
            AbiType::Struct(_)
            | AbiType::Array(_, _)
            | AbiType::Map { .. }
            | AbiType::Set(_, _) => return Ok(true),
            AbiType::Trait(interface) => {
                return Ok(StandardTrait::from_id(&interface.declaration)
                    .is_some_and(StandardTrait::collection));
            }
            AbiType::Tuple(items) | AbiType::StandardEnum { args: items, .. } => {
                items.iter().collect()
            }
            AbiType::Enum(instance) => {
                let Some(payload) = self.enumerations.get(instance) else {
                    return Ok(false);
                };
                payload.clone()
            }
            _ => return Ok(false),
        };
        let interface = intrinsic::applied(kind, vec![]);
        let key = (interface.clone(), receiver.clone());
        if !search.defaults.insert(key.clone()) {
            return Ok(true);
        }
        let result = (|| {
            for member in members {
                if !self.prove(&interface, member, assumptions, search, budget, depth + 1)? {
                    return Ok(false);
                }
            }
            Ok(true)
        })();
        search.defaults.remove(&key);
        result
    }

    pub(super) fn standard_constraint(
        &self,
        actual: &AbiType,
        required: StandardTypeConstraint,
        assumptions: &[GenericBoundAbi],
        search: &mut Search,
        budget: &Budget<'_>,
        depth: usize,
    ) -> Result<bool, TypeTransformError> {
        budget.step(depth)?;
        match required {
            StandardTypeConstraint::HashKey => Ok(self.prove(
                &intrinsic::applied(StandardTrait::Eq, vec![]),
                actual,
                assumptions,
                search,
                budget,
                depth,
            )? && self.prove(
                &intrinsic::applied(StandardTrait::Hash, vec![]),
                actual,
                assumptions,
                search,
                budget,
                depth,
            )?),
            StandardTypeConstraint::Comparable => self.prove(
                &intrinsic::applied(StandardTrait::PartialEq, vec![]),
                actual,
                assumptions,
                search,
                budget,
                depth,
            ),
            _ if matches!(
                actual,
                AbiType::Parameter { .. } | AbiType::Projection { .. }
            ) =>
            {
                Ok(assumptions.iter().any(|bound| {
                    bound.ty == *actual
                        && bound
                            .constraints
                            .contains(&ConstraintAbi::Standard(required))
                }))
            }
            StandardTypeConstraint::OrderedNumber => {
                Ok(matches!(actual, AbiType::Builtin(ty) if ty.number_type().is_some()))
            }
            StandardTypeConstraint::SignedNumber => Ok(
                matches!(actual, AbiType::Builtin(ty) if ty.integer_layout().is_some_and(|(_, signed)| signed) || matches!(ty, BuiltinType::F32 | BuiltinType::F64)),
            ),
        }
    }
}

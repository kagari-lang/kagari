use crate::{
    language::{Protocol, primitive as intrinsic},
    scalar::BuiltinType,
    standard::surface::{StandardEnum, StandardTypeConstraint},
    types::{
        Constraint, GenericBound, Ty,
        proofs::{Budget, ProofCatalog, search::Search},
        substitution::TypeTransformError,
    },
};

impl ProofCatalog<'_> {
    pub(super) fn structural(
        &self,
        kind: Protocol,
        receiver: &Ty,
        assumptions: &[GenericBound],
        search: &mut Search,
        budget: &Budget<'_>,
        depth: usize,
    ) -> Result<bool, TypeTransformError> {
        budget.step(depth)?;
        if matches!(kind, Protocol::PartialOrd | Protocol::Ord) {
            return Ok(match receiver {
                Ty::Builtin(BuiltinType::F32 | BuiltinType::F64) => kind == Protocol::PartialOrd,
                Ty::Builtin(_)
                | Ty::StandardEnum {
                    kind: StandardEnum::Ordering,
                    ..
                } => true,
                _ => false,
            });
        }
        if !kind.equality_protocol() && !matches!(kind, Protocol::Debug | Protocol::Display) {
            return Ok(false);
        }
        if kind == Protocol::PartialEq
            && assumptions
                .iter()
                .filter(|bound| bound.ty == *receiver)
                .flat_map(|bound| &bound.constraints)
                .any(|bound| *bound == Constraint::Trait(intrinsic::applied(Protocol::Eq, vec![])))
        {
            return Ok(true);
        }
        if matches!(receiver, Ty::Struct(_) | Ty::NativeObject(_) | Ty::Enum(_))
            && matches!(kind, Protocol::Eq | Protocol::Hash)
            && self.explicit(
                &intrinsic::applied(Protocol::PartialEq, vec![]),
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
        let members: Vec<&Ty> = match receiver {
            Ty::Builtin(ty) => {
                return Ok(!matches!(kind, Protocol::Eq | Protocol::Hash)
                    || !matches!(ty, BuiltinType::F32 | BuiltinType::F64));
            }
            _ if kind == Protocol::Display => return Ok(false),
            Ty::Host(_) => return Ok(kind == Protocol::Debug),
            Ty::Enum(_) if kind == Protocol::Debug => return Ok(true),
            Ty::Struct(_)
            | Ty::NativeObject(_)
            | Ty::Array(_, _)
            | Ty::Map { .. }
            | Ty::Set(_, _) => return Ok(true),
            Ty::Trait(interface) => {
                return Ok(
                    Protocol::from_id(&interface.declaration).is_some_and(Protocol::collection)
                );
            }
            Ty::Tuple(items) | Ty::StandardEnum { args: items, .. } => items.iter().collect(),
            Ty::Enum(instance) => {
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
        actual: &Ty,
        required: StandardTypeConstraint,
        assumptions: &[GenericBound],
        search: &mut Search,
        budget: &Budget<'_>,
        depth: usize,
    ) -> Result<bool, TypeTransformError> {
        budget.step(depth)?;
        match required {
            StandardTypeConstraint::HashKey => Ok(self.prove(
                &intrinsic::applied(Protocol::Eq, vec![]),
                actual,
                assumptions,
                search,
                budget,
                depth,
            )? && self.prove(
                &intrinsic::applied(Protocol::Hash, vec![]),
                actual,
                assumptions,
                search,
                budget,
                depth,
            )?),
            StandardTypeConstraint::Comparable => self.prove(
                &intrinsic::applied(Protocol::PartialEq, vec![]),
                actual,
                assumptions,
                search,
                budget,
                depth,
            ),
            _ if matches!(actual, Ty::Parameter { .. } | Ty::Projection { .. }) => {
                Ok(assumptions.iter().any(|bound| {
                    bound.ty == *actual
                        && bound.constraints.contains(&Constraint::Standard(required))
                }))
            }
            StandardTypeConstraint::OrderedNumber => {
                Ok(matches!(actual, Ty::Builtin(ty) if required.accepts_builtin_number(*ty)))
            }
            StandardTypeConstraint::SignedNumber => {
                Ok(matches!(actual, Ty::Builtin(ty) if required.accepts_builtin_number(*ty)))
            }
        }
    }
}

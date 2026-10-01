use crate::{
    standard::{intrinsic, traits::StandardTrait},
    types::{
        AbiType,
        proofs::implementation::Implementation,
        proofs::{Budget, ProofCatalog},
        substitution::TypeTransformError,
    },
};
use kagari_common::cancellation::CancellationToken;
use std::iter;

impl ProofCatalog<'_> {
    pub fn overrides_valid(&self, cancel: &CancellationToken) -> Result<bool, TypeTransformError> {
        let budget = Budget::new(cancel);
        for host in &self.hosts {
            for implementation in &host.trait_implementations {
                budget.step(0)?;
                if let Some(kind) = StandardTrait::from_id(&implementation.trait_id) {
                    if kind.equality_protocol() || !kind.host_implementable() {
                        return Ok(false);
                    }
                    if kind.iteration()
                        && self.iteration_conflict(
                            kind,
                            &AbiType::Host(host.id.clone()),
                            &budget,
                        )?
                    {
                        return Ok(false);
                    }
                }
            }
        }
        for table in &self.implementations {
            budget.step(0)?;
            if !self.table_override_valid(table, &budget)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn iteration_conflict(
        &self,
        kind: StandardTrait,
        receiver: &AbiType,
        budget: &Budget<'_>,
    ) -> Result<bool, TypeTransformError> {
        let other = if kind == StandardTrait::Iterator {
            StandardTrait::Iterable
        } else {
            StandardTrait::Iterator
        };
        for table in &self.implementations {
            budget.step(0)?;
            if let Some(applied) = table.interface()
                && StandardTrait::from_id(&applied.declaration) == Some(other)
                && overlapping(table.receiver(), receiver)
            {
                return Ok(true);
            }
        }
        for host in &self.hosts {
            for implementation in &host.trait_implementations {
                budget.step(0)?;
                if StandardTrait::from_id(&implementation.trait_id) == Some(other)
                    && *receiver == AbiType::Host(host.id.clone())
                {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    fn table_override_valid(
        &self,
        table: &Implementation<'_>,
        budget: &Budget<'_>,
    ) -> Result<bool, TypeTransformError> {
        let Some(interface) = table.interface() else {
            return Ok(false);
        };
        let Some(kind) = StandardTrait::from_id(&interface.declaration) else {
            return Ok(true);
        };
        if kind.iteration() && self.iteration_conflict(kind, table.receiver(), budget)? {
            return Ok(false);
        }
        if kind.reverse_conversion() {
            return Ok(false);
        }
        if kind == StandardTrait::From
            && interface.arguments.first().is_some_and(|input| {
                input == table.receiver()
                    || match (input, table.receiver()) {
                        (AbiType::Parameter { .. }, target) => {
                            !occurs_in_constructor(input, target)
                        }
                        (source, AbiType::Parameter { .. }) => {
                            !occurs_in_constructor(table.receiver(), source)
                        }
                        _ => overlapping(input, table.receiver()),
                    }
            })
        {
            return Ok(false);
        }
        if kind.conversion() {
            if iter::once(table.receiver())
                .chain(&interface.arguments)
                .any(|ty| matches!(ty, AbiType::Host(_)))
            {
                return Ok(false);
            }
            return Ok(iter::once(table.receiver()).chain(&interface.arguments).any(|ty| matches!(ty, AbiType::Struct(n) | AbiType::Enum(n) if n.declaration.module == table.declaration().module)));
        }
        if kind.host_implementable() {
            return Ok(true);
        }
        let (AbiType::Struct(nominal) | AbiType::Enum(nominal)) = table.receiver() else {
            return Ok(interface.declaration.module == table.declaration().module);
        };
        if nominal.declaration.module != table.declaration().module {
            return Ok(false);
        }
        if !kind.equality_protocol() {
            return Ok(true);
        }
        for required in [StandardTrait::PartialEq, StandardTrait::Eq] {
            if kind == StandardTrait::PartialEq
                || kind == StandardTrait::Eq && required == StandardTrait::Eq
            {
                continue;
            }
            // All nested checks share the override audit's work/cancellation budget.
            if self.explicit(
                &intrinsic::applied(required, vec![]),
                table.receiver(),
                table.bounds(),
                &mut Default::default(),
                budget,
                0,
            )? != 1
            {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

fn overlapping(left: &AbiType, right: &AbiType) -> bool {
    if left == right {
        return true;
    }
    if left.is_concrete() && right.is_concrete() {
        return false;
    }
    match (left, right) {
        (AbiType::Struct(left), AbiType::Struct(right))
        | (AbiType::Enum(left), AbiType::Enum(right)) => left.declaration == right.declaration,
        (AbiType::StandardEnum { kind: left, .. }, AbiType::StandardEnum { kind: right, .. }) => {
            left == right
        }
        (AbiType::Tuple(left), AbiType::Tuple(right)) => left.len() == right.len(),
        (AbiType::Function { params: left, .. }, AbiType::Function { params: right, .. }) => {
            left.len() == right.len()
        }
        (AbiType::Range(_, left), AbiType::Range(_, right)) => left == right,
        (AbiType::Iter(_), AbiType::Iter(_))
        | (AbiType::Array(_, _), AbiType::Array(_, _))
        | (AbiType::Set(_, _), AbiType::Set(_, _))
        | (AbiType::Map { .. }, AbiType::Map { .. }) => true,
        _ => false,
    }
}

fn occurs_in_constructor(parameter: &AbiType, ty: &AbiType) -> bool {
    let mut pending = vec![ty];
    while let Some(ty) = pending.pop() {
        if ty == parameter {
            return true;
        }
        match ty {
            AbiType::Struct(n) | AbiType::Enum(n) => pending.extend(&n.arguments),
            AbiType::Tuple(items) | AbiType::StandardEnum { args: items, .. } => {
                pending.extend(items)
            }
            AbiType::Array(item, _)
            | AbiType::Set(item, _)
            | AbiType::Iter(item)
            | AbiType::Range(item, _) => pending.push(item),
            AbiType::Map { key, value, .. } => pending.extend([key.as_ref(), value.as_ref()]),
            AbiType::Function { params, result } => {
                pending.extend(params);
                pending.push(result);
            }
            _ => {}
        }
    }
    false
}

use crate::{
    language::{Protocol, primitive as intrinsic},
    library::namespaces,
    types::{
        Ty,
        conversion::ConversionAdapter,
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
                if let Some(kind) = Protocol::from_id(&implementation.trait_id) {
                    if kind.equality_protocol() || !kind.host_implementable() {
                        return Ok(false);
                    }
                    if kind.iteration()
                        && self.iteration_conflict(kind, &Ty::Host(host.id.clone()), &budget)?
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
        kind: Protocol,
        receiver: &Ty,
        budget: &Budget<'_>,
    ) -> Result<bool, TypeTransformError> {
        let other = if kind == Protocol::Iterator {
            Protocol::Iterable
        } else {
            Protocol::Iterator
        };
        for table in &self.implementations {
            budget.step(0)?;
            if let Some(applied) = table.interface()
                && Protocol::from_id(&applied.declaration) == Some(other)
                && overlapping(table.receiver(), receiver)
            {
                return Ok(true);
            }
        }
        for host in &self.hosts {
            for implementation in &host.trait_implementations {
                budget.step(0)?;
                if Protocol::from_id(&implementation.trait_id) == Some(other)
                    && *receiver == Ty::Host(host.id.clone())
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
        if self
            .trait_contract(&interface.declaration)
            .is_some_and(|contract| contract.storage_access.is_some())
        {
            return Ok(match table.receiver() {
                Ty::NativeObject(nominal) | Ty::Struct(nominal) | Ty::Enum(nominal) => {
                    nominal.declaration.module == table.declaration().module
                }
                Ty::Array(_, _) | Ty::Map { .. } | Ty::Set(_, _) => {
                    namespaces::receiver_owner(table.receiver()).as_ref()
                        == Some(&table.declaration().module)
                }
                _ => false,
            });
        }
        let adapter = self
            .trait_contract(&interface.declaration)
            .and_then(|contract| contract.conversion_adapter.as_ref());
        let kind = Protocol::from_id(&interface.declaration);
        if matches!(adapter, Some(ConversionAdapter::Reverse { .. })) {
            return Ok(false);
        }
        if kind == Some(Protocol::From)
            && interface.arguments.first().is_some_and(|input| {
                input == table.receiver()
                    || match (input, table.receiver()) {
                        (Ty::Parameter { .. }, target) => !occurs_in_constructor(input, target),
                        (source, Ty::Parameter { .. }) => {
                            !occurs_in_constructor(table.receiver(), source)
                        }
                        _ => overlapping(input, table.receiver()),
                    }
            })
        {
            return Ok(false);
        }
        if kind == Some(Protocol::From)
            || matches!(adapter, Some(ConversionAdapter::CheckedNumeric { .. }))
        {
            if iter::once(table.receiver())
                .chain(&interface.arguments)
                .any(|ty| matches!(ty, Ty::Host(_)))
            {
                return Ok(false);
            }
            let owner = &table.declaration().module;
            return Ok(interface.declaration.module == *owner
                || iter::once(table.receiver())
                    .chain(&interface.arguments)
                    .any(|ty| namespaces::receiver_owner(ty).as_ref() == Some(owner)));
        }
        let Some(kind) = kind else {
            return Ok(true);
        };
        if kind.iteration() && self.iteration_conflict(kind, table.receiver(), budget)? {
            return Ok(false);
        }
        if kind.host_implementable() {
            return Ok(true);
        }
        let (Ty::NativeObject(nominal) | Ty::Struct(nominal) | Ty::Enum(nominal)) =
            table.receiver()
        else {
            return Ok(namespaces::receiver_owner(table.receiver()).as_ref()
                == Some(&table.declaration().module));
        };
        if nominal.declaration.module != table.declaration().module {
            return Ok(false);
        }
        if !kind.equality_protocol() {
            return Ok(true);
        }
        for required in [Protocol::PartialEq, Protocol::Eq] {
            if kind == Protocol::PartialEq || kind == Protocol::Eq && required == Protocol::Eq {
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

fn overlapping(left: &Ty, right: &Ty) -> bool {
    if left == right {
        return true;
    }
    if left.is_concrete() && right.is_concrete() {
        return false;
    }
    match (left, right) {
        (Ty::Struct(left), Ty::Struct(right))
        | (Ty::NativeObject(left), Ty::NativeObject(right))
        | (Ty::Enum(left), Ty::Enum(right)) => left.declaration == right.declaration,
        (Ty::StandardEnum { kind: left, .. }, Ty::StandardEnum { kind: right, .. }) => {
            left == right
        }
        (Ty::Tuple(left), Ty::Tuple(right)) => left.len() == right.len(),
        (Ty::Function { params: left, .. }, Ty::Function { params: right, .. }) => {
            left.len() == right.len()
        }
        (Ty::Range(_, left), Ty::Range(_, right)) => left == right,
        (Ty::Iter(_), Ty::Iter(_))
        | (Ty::Array(_, _), Ty::Array(_, _))
        | (Ty::Set(_, _), Ty::Set(_, _))
        | (Ty::Map { .. }, Ty::Map { .. }) => true,
        _ => false,
    }
}

fn occurs_in_constructor(parameter: &Ty, ty: &Ty) -> bool {
    let mut pending = vec![ty];
    while let Some(ty) = pending.pop() {
        if ty == parameter {
            return true;
        }
        match ty {
            Ty::Struct(n) | Ty::NativeObject(n) | Ty::Enum(n) => pending.extend(&n.arguments),
            Ty::Tuple(items) | Ty::StandardEnum { args: items, .. } => pending.extend(items),
            Ty::Array(item, _) | Ty::Set(item, _) | Ty::Iter(item) | Ty::Range(item, _) => {
                pending.push(item)
            }
            Ty::Map { key, value, .. } => pending.extend([key.as_ref(), value.as_ref()]),
            Ty::Function { params, result } => {
                pending.extend(params);
                pending.push(result);
            }
            _ => {}
        }
    }
    false
}

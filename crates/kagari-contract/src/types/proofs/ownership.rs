use crate::{
    language::primitive as intrinsic,
    types::proofs::{Budget, ProofCatalog, implementation::Implementation},
};
use kagari_common::cancellation::CancellationToken;
use kagari_types::{
    callable::CallableImplementation,
    declaration::{
        conversion::{is_library_forwarding_trait, library_forwarding_header_valid},
        ownership::trait_impl_orphan_error,
    },
    language::Protocol,
    ty::{Ty, substitution::TypeTransformError},
};

impl ProofCatalog<'_> {
    pub fn overrides_valid(&self, cancel: &CancellationToken) -> Result<bool, TypeTransformError> {
        let budget = Budget::new(cancel);

        for table in self.implementations.iter() {
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
        for table in self.implementations.iter() {
            budget.step(0)?;
            if let Some(applied) = table.interface()
                && Protocol::from_definition(&applied.declaration, &self.definitions) == Some(other)
                && overlapping(table.receiver(), receiver)
            {
                return Ok(true);
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
        if trait_impl_orphan_error(
            self.module(*table.declaration())?,
            &self.definitions,
            interface.declaration,
            table.receiver(),
            &interface.arguments,
        )
        .is_some()
        {
            return Ok(false);
        }
        if is_library_forwarding_trait(&self.definitions, interface.declaration) {
            let [method] = table.methods() else {
                return Ok(false);
            };
            let name = if interface.associated_types.is_empty() {
                "into"
            } else {
                "try_into"
            };
            return Ok(method.name == name
                && method.implementation == CallableImplementation::Script
                && library_forwarding_header_valid(
                    &self.definitions,
                    *table.declaration(),
                    table.parameters(),
                    table.receiver(),
                    interface,
                    table.bounds(),
                ));
        }
        let kind = Protocol::from_definition(&interface.declaration, &self.definitions);
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

        let Some(kind) = kind else {
            return Ok(true);
        };
        if kind.iteration() && self.iteration_conflict(kind, table.receiver(), budget)? {
            return Ok(false);
        }
        if !kind.host_implementable() && matches!(table.receiver(), Ty::Host(_))
            || kind != Protocol::From && matches!(table.receiver(), Ty::Parameter { .. })
            || matches!(kind, Protocol::Debug | Protocol::Display)
                && matches!(table.receiver(), Ty::Builtin(_))
        {
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
                &intrinsic::applied(required, vec![], &self.definitions)?,
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

        (Ty::Tuple(left), Ty::Tuple(right)) => left.len() == right.len(),
        (Ty::Function { params: left, .. }, Ty::Function { params: right, .. }) => {
            left.len() == right.len()
        }
        (Ty::Range(_, left), Ty::Range(_, right)) => left == right,
        (Ty::Array(_), Ty::Array(_)) => true,
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
            Ty::Tuple(items) => pending.extend(items),
            Ty::Array(item) | Ty::Range(item, _) => pending.push(item),

            Ty::Function { params, result } => {
                pending.extend(params);
                pending.push(result);
            }
            _ => {}
        }
    }
    false
}

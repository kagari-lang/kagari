use {
    crate::{
        language::primitive as intrinsic,
        types::proofs::{Budget, ProofCatalog, search::Search},
    },
    kagari_types::{
        language::{Protocol, binding},
        scalar::BuiltinType,
        ty::{GenericBound, Ty, substitution::TypeTransformError},
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
                Ty::Builtin(_) => true,
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
                .any(|interface| {
                    Protocol::from_definition(&interface.declaration, &self.definitions)
                        == Some(Protocol::Eq)
                        && interface.arguments.is_empty()
                        && interface.associated_types.is_empty()
                })
        {
            return Ok(true);
        }
        let members: Vec<&Ty> = match receiver {
            Ty::Builtin(ty) => {
                return Ok(!matches!(kind, Protocol::Eq | Protocol::Hash)
                    || !matches!(ty, BuiltinType::F32 | BuiltinType::F64));
            }
            _ if kind == Protocol::Display => return Ok(false),
            Ty::Host(_) => return Ok(kind == Protocol::Debug),
            Ty::Enum(_) if kind == Protocol::Debug => return Ok(true),
            Ty::Struct(_) | Ty::NativeObject(_) | Ty::Map { .. } | Ty::Set(_, _) => {
                return Ok(kind == Protocol::Debug);
            }
            Ty::Array(_) => return Ok(kind == Protocol::Debug),
            Ty::Tuple(items) => items.iter().collect(),
            Ty::Enum(instance)
                if binding::member_comparison(&instance.declaration, &self.definitions) =>
            {
                let Some(payload) = self.enumerations.get(instance) else {
                    // Core member comparison requires complete layout evidence.
                    return Err(TypeTransformError::InvalidContract);
                };
                payload.clone()
            }
            _ => return Ok(false),
        };
        let interface = intrinsic::applied(kind, vec![], &self.definitions)?;
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
}

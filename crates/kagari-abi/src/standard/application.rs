//! Bind generated standard declarations to known executable operand types.
use crate::standard::declarations::ApiType;
use crate::standard::resolve::Arguments;
use crate::standard::traits::StandardTrait;
use crate::types::AbiType;
use crate::types::substitution::{MAX_TYPE_NODES, TypeSubstitution, TypeTransformError};
use kagari_common::cancellation::CancellationToken;
use kagari_common::collection::CollectionAccess;
use std::collections::BTreeSet;

pub struct StandardArguments {
    declared: BTreeSet<&'static str>,
    values: Arguments,
}

impl StandardArguments {
    pub fn new(parameters: &[&'static str]) -> Self {
        Self {
            declared: parameters.iter().copied().collect(),
            values: Arguments::new(),
        }
    }

    /// Bind only declared parameters. Later occurrences retain the first known
    /// binding; the caller checks each instantiated operand's permitted access flow.
    pub fn bind(
        &mut self,
        template: &ApiType,
        actual: &AbiType,
        cancel: &CancellationToken,
    ) -> Result<(), TypeTransformError> {
        let mut pending = vec![(template, actual)];
        let mut remaining = MAX_TYPE_NODES;
        while let Some((template, actual)) = pending.pop() {
            cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
            if remaining == 0 {
                return Err(TypeTransformError::LimitExceeded);
            }
            remaining -= 1;
            match (template, actual) {
                (ApiType::Named(name, params), AbiType::Trait(interface))
                    if StandardTrait::from_id(&interface.declaration).is_some_and(
                        |kind| match *name {
                            "ArrayList" => {
                                matches!(kind, StandardTrait::List | StandardTrait::MutableList)
                            }
                            "LinkedHashMap" => {
                                matches!(kind, StandardTrait::Map | StandardTrait::MutableMap)
                            }
                            "LinkedHashSet" => {
                                matches!(kind, StandardTrait::Set | StandardTrait::MutableSet)
                            }
                            _ => kind.name() == *name,
                        },
                    ) && params.len() == interface.arguments.len() =>
                {
                    pending.extend(params.iter().zip(&interface.arguments).rev())
                }
                (ApiType::Array(item), AbiType::Trait(interface))
                    if matches!(
                        StandardTrait::from_id(&interface.declaration),
                        Some(StandardTrait::List | StandardTrait::MutableList)
                    ) =>
                {
                    if let [actual] = interface.arguments.as_slice() {
                        pending.push((item, actual));
                    }
                }
                (ApiType::Named(name, []), actual) if self.declared.contains(name) => {
                    if !self.values.contains_key(name) {
                        self.values
                            .insert(name, TypeSubstitution::default().apply(actual, cancel)?);
                    }
                }
                (ApiType::Named(name, [item]), AbiType::Range(actual, kind))
                    if *name == kind.name() =>
                {
                    pending.push((item, actual))
                }
                (ApiType::Array(item), AbiType::Array(actual, _)) => pending.push((item, actual)),
                (ApiType::Named("ArrayList", [item]), AbiType::Array(actual, _))
                | (ApiType::Named("Set" | "LinkedHashSet", [item]), AbiType::Set(actual, _))
                | (ApiType::Named("Iter", [item]), AbiType::Iter(actual)) => {
                    pending.push((item, actual))
                }
                (ApiType::Tuple(items), AbiType::Tuple(actual)) if items.len() == actual.len() => {
                    pending.extend(items.iter().zip(actual).rev())
                }
                (
                    ApiType::Function(params, result),
                    AbiType::Function {
                        params: actual,
                        result: output,
                    },
                ) if params.len() == actual.len() => {
                    pending.push((result, output));
                    pending.extend(params.iter().zip(actual).rev());
                }
                (
                    ApiType::Named("Map" | "LinkedHashMap", [key, value]),
                    AbiType::Map {
                        key: actual,
                        value: output,
                        ..
                    },
                ) => pending.extend([(value, output.as_ref()), (key, actual.as_ref())]),
                (ApiType::Named(name, params), AbiType::StandardEnum { kind, args })
                    if *name == kind.spec().name && params.len() == args.len() =>
                {
                    pending.extend(params.iter().zip(args).rev())
                }
                _ => {}
            }
            if pending.len() > remaining {
                return Err(TypeTransformError::LimitExceeded);
            }
        }
        Ok(())
    }

    pub fn resolve(
        &self,
        template: &ApiType,
        cancel: &CancellationToken,
    ) -> Result<Option<AbiType>, TypeTransformError> {
        template
            .resolve(&self.values)
            .map(|ty| TypeSubstitution::default().apply(&ty, cancel))
            .transpose()
    }

    /// Access is known for a storage constructor even when its element is unknown.
    pub fn collection_access(&self, template: &ApiType) -> Option<CollectionAccess> {
        match template {
            ApiType::Named(name, _) if self.values.contains_key(name) => {
                self.values[name].collection_access()
            }
            ApiType::Named("ArrayList" | "LinkedHashMap" | "LinkedHashSet", _) => {
                Some(CollectionAccess::Mutable)
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scalar::BuiltinType;

    #[test]
    fn repeated_parameters_preserve_the_first_binding_for_access_checks() {
        let item = ApiType::Named("T", &[]);
        let readonly = AbiType::Array(
            Box::new(AbiType::Builtin(BuiltinType::I32)),
            CollectionAccess::ReadOnly,
        );
        let mutable = AbiType::Array(
            Box::new(AbiType::Builtin(BuiltinType::I32)),
            CollectionAccess::Mutable,
        );
        let mut arguments = StandardArguments::new(&["T"]);
        let cancel = CancellationToken::default();
        arguments.bind(&item, &readonly, &cancel).unwrap();
        arguments.bind(&item, &mutable, &cancel).unwrap();
        assert_eq!(
            arguments.resolve(&item, &cancel).unwrap(),
            Some(readonly.clone())
        );
        assert!(mutable.can_weaken_to(&readonly));
    }

    #[test]
    fn unknown_elements_do_not_erase_storage_access_and_binding_is_bounded() {
        let template = ApiType::Named("ArrayList", &[ApiType::Named("T", &[])]);
        let mut arguments = StandardArguments::new(&["T"]);
        let cancel = CancellationToken::default();
        assert_eq!(arguments.resolve(&template, &cancel).unwrap(), None);
        assert_eq!(
            arguments.collection_access(&template),
            Some(CollectionAccess::Mutable)
        );
        let actual = AbiType::Array(
            Box::new(AbiType::Builtin(BuiltinType::Bool)),
            CollectionAccess::Mutable,
        );
        arguments.bind(&template, &actual, &cancel).unwrap();
        assert_eq!(
            arguments.resolve(&template, &cancel).unwrap(),
            Some(actual.clone())
        );
        cancel.cancel();
        assert_eq!(
            arguments.bind(&template, &actual, &cancel),
            Err(TypeTransformError::Cancelled)
        );
    }
}

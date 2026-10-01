//! The default array API is authored once as native registration declarations.
use crate::{NativeApi, native::array::handlers};
use kagari_abi::{
    callable::CallableImplementation,
    native_api::{NativeImplementation, NativeModule},
    native_import::binding_id,
    scalar::BuiltinType,
    standard::{
        surface::StandardEnum,
        traits::{self, StandardTrait},
    },
    types::{
        AbiType, FunctionAbi, GenericParameterAbi, NominalAbiType, ParameterAbi, TraitAbi, TypeAbi,
        TypeAbiKind, native::NativeTypeConstructor,
    },
};
use kagari_common::{
    collection::CollectionAccess,
    identity::{DefinitionId, DefinitionKind, associated_type_id},
};
use std::collections::BTreeMap;

fn function(
    name: &str,
    params: Vec<(&str, AbiType)>,
    result: AbiType,
    implementation: CallableImplementation,
) -> FunctionAbi {
    FunctionAbi {
        method_policy: Default::default(),
        name: name.into(),
        implementation,
        generic_params: vec![],
        bounds: vec![],
        params: params
            .into_iter()
            .map(|(name, ty)| ParameterAbi {
                name: name.into(),
                ty,
                mutable: false,
            })
            .collect(),
        return_type: result,
    }
}
fn parameter(owner: &DefinitionId) -> GenericParameterAbi {
    GenericParameterAbi {
        owner: owner.clone(),
        position: 0,
    }
}
fn nominal(declaration: DefinitionId, argument: AbiType) -> NominalAbiType {
    NominalAbiType {
        declaration,
        arguments: vec![argument],
        associated_types: BTreeMap::new(),
    }
}
fn trait_definition(
    name: &str,
    generic: GenericParameterAbi,
    supertraits: Vec<NominalAbiType>,
    methods: Vec<FunctionAbi>,
) -> TraitAbi {
    TraitAbi {
        name: name.into(),
        generic_params: vec![generic],
        supertraits,
        methods,
        bounds: vec![],
        associated_types: vec![],
        associated_consts: vec![],
    }
}

pub fn standard_library() -> NativeApi {
    let identity = traits::identity(StandardTrait::List).module;
    let mut module = NativeModule::new(identity);
    let unit = AbiType::Builtin(BuiltinType::Unit);
    let usize_type = AbiType::Builtin(BuiltinType::USize);
    let array = |t| AbiType::Array(Box::new(t), CollectionAccess::Mutable);
    let storage_owner = module.definition(DefinitionKind::AssociatedType, "ArrayList");
    module.types.push(TypeAbi {
        name: "ArrayList".into(),
        kind: TypeAbiKind::Native(NativeTypeConstructor::Array),
        generic_params: vec![parameter(&storage_owner)],
        bounds: vec![],
        fields: vec![],
        variants: vec![],
    });
    module.documentation.insert(
        storage_owner,
        "Shared mutable array storage. Read-only List views share its identity.".into(),
    );

    let owner = module.implementation_id(0);
    let generic = parameter(&owner);
    let t = generic.as_type();
    let mut methods = vec![
        function(
            "new",
            vec![],
            array(t.clone()),
            CallableImplementation::Native(binding_id(&module.identity, "array_new")),
        ),
        function(
            "len",
            vec![("self", array(t.clone()))],
            usize_type.clone(),
            CallableImplementation::Native(binding_id(&module.identity, "array_len")),
        ),
        function(
            "push",
            vec![("self", array(t.clone())), ("value", t.clone())],
            unit.clone(),
            CallableImplementation::Native(binding_id(&module.identity, "array_push")),
        ),
        function(
            "from_fn",
            vec![
                ("count", usize_type.clone()),
                (
                    "make",
                    AbiType::Function {
                        params: vec![usize_type.clone()],
                        result: Box::new(t.clone()),
                    },
                ),
            ],
            array(t.clone()),
            CallableImplementation::Native(binding_id(&module.identity, "array_from_fn")),
        ),
    ];
    for method in &mut methods {
        method.generic_params = vec![generic.clone()];
    }
    for (name, doc) in [
        (
            "new",
            "Allocate an empty array. The item type must be inferable from context.",
        ),
        ("len", "Return the current slot count."),
        (
            "push",
            "Append a value. Allocation and iteration guards are checked before mutation.",
        ),
        (
            "from_fn",
            "Build slots by calling make once per index in ascending order.",
        ),
    ] {
        module
            .documentation
            .insert(NativeModule::method_id(&owner, name), doc.into());
    }
    module.implementations.push(NativeImplementation {
        generic_params: vec![generic],
        trait_type: None,
        for_type: array(t),
        methods,
    });

    let list_id = module.definition(DefinitionKind::Trait, "List");
    let list_generic = parameter(&list_id);
    let t = list_generic.as_type();
    let index_id = traits::identity(StandardTrait::Index);
    let mut index = nominal(index_id.clone(), usize_type.clone());
    index
        .associated_types
        .insert(associated_type_id(&index_id, "Output"), t.clone());
    module.traits.push(trait_definition(
        "List",
        list_generic,
        vec![index],
        vec![
            function(
                "len",
                vec![("self", AbiType::SelfType(list_id.clone()))],
                usize_type.clone(),
                CallableImplementation::Required,
            ),
            function(
                "get",
                vec![
                    ("self", AbiType::SelfType(list_id.clone())),
                    ("index", usize_type.clone()),
                ],
                AbiType::StandardEnum {
                    kind: StandardEnum::Option,
                    args: vec![t],
                },
                CallableImplementation::Required,
            ),
        ],
    ));
    let mutable_id = module.definition(DefinitionKind::Trait, "MutableList");
    let mutable_generic = parameter(&mutable_id);
    let t = mutable_generic.as_type();
    module.traits.push(trait_definition(
        "MutableList",
        mutable_generic,
        vec![nominal(list_id.clone(), t.clone())],
        vec![function(
            "set",
            vec![
                ("self", AbiType::SelfType(mutable_id.clone())),
                ("index", usize_type),
                ("value", t),
            ],
            unit,
            CallableImplementation::Required,
        )],
    ));
    module.documentation.insert(
        list_id.clone(),
        "Shared read access with checked indexing.".into(),
    );
    module.documentation.insert(
        mutable_id.clone(),
        "Mutable list access through declared methods.".into(),
    );
    for (id, name, doc) in [
        (&list_id, "len", "Return the current slot count."),
        (
            &list_id,
            "get",
            "Return the addressed value, or None when index is outside the array.",
        ),
        (
            &mutable_id,
            "set",
            "Replace a valid slot, trapping before mutation for an invalid index.",
        ),
    ] {
        module
            .documentation
            .insert(NativeModule::method_id(id, name), doc.into());
    }

    for (trait_id, bindings) in [
        (list_id, vec![("len", "array_len"), ("get", "array_get")]),
        (mutable_id, vec![("set", "array_set")]),
    ] {
        let generic = parameter(&module.implementation_id(module.implementations.len()));
        let t = generic.as_type();
        let bindings = bindings
            .iter()
            .map(|(method, entry)| (*method, binding_id(&module.identity, entry)))
            .collect::<Vec<_>>();
        module
            .implement_trait(
                nominal(trait_id, t.clone()),
                array(t),
                vec![generic],
                &bindings,
            )
            .expect("bundled trait implementation");
    }
    let handlers = handlers(&module.identity);
    NativeApi::new(vec![module], handlers).expect("bundled native API is valid")
}

#[cfg(all(feature = "source", feature = "native"))]
mod array_operations;

#[cfg(all(feature = "source", feature = "native"))]
mod associated_constants;

#[cfg(all(feature = "source", feature = "native"))]
mod associated_types;

#[cfg(all(feature = "source", feature = "native"))]
mod callable_traits;

#[cfg(all(feature = "source", feature = "native"))]
mod collection_access;

#[cfg(feature = "source")]
mod collection_callbacks;

#[cfg(all(feature = "source", feature = "native"))]
mod collection_interfaces;

#[cfg(all(feature = "source", feature = "native"))]
mod conversion_traits;

#[cfg(all(feature = "source", feature = "native"))]
mod default_methods;

#[cfg(feature = "source")]
mod enum_payloads;

#[cfg(all(feature = "source", feature = "native"))]
mod generic_associated_types;

#[cfg(all(feature = "source", feature = "native"))]
mod instantiation;

#[cfg(all(feature = "source", feature = "native"))]
mod interface_identity;

#[cfg(all(feature = "source", feature = "native"))]
mod iteration_traits;

#[cfg(feature = "source")]
mod library_namespaces;

#[cfg(feature = "source")]
mod list_algorithms;

#[cfg(feature = "source")]
mod native_enums;

#[cfg(all(feature = "source", feature = "native"))]
mod never;

#[cfg(all(feature = "source", feature = "native"))]
mod numeric_operations;

#[cfg(all(feature = "source", feature = "native"))]
mod operator_traits;

#[cfg(all(feature = "source", feature = "native"))]
mod result_option;

#[cfg(all(feature = "source", feature = "native"))]
mod standard_traits;

#[cfg(all(feature = "source", feature = "native"))]
mod string_interpolation;

#[cfg(feature = "source")]
mod string_methods;

#[cfg(all(feature = "source", feature = "native"))]
mod syntax_examples;

#[cfg(all(feature = "source", feature = "native"))]
mod trait_inheritance;

#[cfg(feature = "source")]
mod try_protocols;

#[cfg(all(feature = "source", feature = "native"))]
mod type_inference;

#[cfg(feature = "source")]
mod local_functions;

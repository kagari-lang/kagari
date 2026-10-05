use crate::{
    Runtime,
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    native::{
        binding::NativeResult,
        conversion::{KagariType, arguments::KagariArguments},
        function_handle::PinnedFunction,
        methods::{InherentMember, InherentType, Method},
    },
};

impl Runtime {
    pub fn bind_method<A: KagariArguments + 'static, R: KagariType + 'static>(
        &self,
        object_type: &impl InherentType,
        name: &str,
    ) -> NativeResult<Method<A, R>> {
        self.bind_method_application(object_type, name, &[])
    }

    pub fn bind_method_declaration<A: KagariArguments + 'static, R: KagariType + 'static>(
        &self,
        member: &InherentMember,
    ) -> NativeResult<Method<A, R>> {
        self.bind_method_application_declaration(member, &[])
    }

    /// Bind an installed application; type arguments here belong to the method.
    pub fn bind_method_application<A: KagariArguments + 'static, R: KagariType + 'static>(
        &self,
        object_type: &impl InherentType,
        name: &str,
        arguments: &[TypeArgument],
    ) -> NativeResult<Method<A, R>> {
        self.bind_method_application_declaration(&object_type.method(name)?, arguments)
    }

    pub fn bind_method_application_declaration<
        A: KagariArguments + 'static,
        R: KagariType + 'static,
    >(
        &self,
        member: &InherentMember,
        arguments: &[TypeArgument],
    ) -> NativeResult<Method<A, R>> {
        if !member.has_receiver {
            return Err(RuntimeError::module_validation(
                "associated function has no receiver",
            ));
        }
        Ok(Method {
            receiver: member.applied.clone(),
            function: self.bind_member_entry(member, arguments)?,
        })
    }

    /// Associated constructors and other static members use ordinary callable handles.
    pub fn bind_associated_function<A: KagariArguments + 'static, R: KagariType + 'static>(
        &self,
        object_type: &impl InherentType,
        name: &str,
    ) -> NativeResult<PinnedFunction<A, R>> {
        self.bind_associated_function_application(object_type, name, &[])
    }

    pub fn bind_associated_function_declaration<
        A: KagariArguments + 'static,
        R: KagariType + 'static,
    >(
        &self,
        member: &InherentMember,
    ) -> NativeResult<PinnedFunction<A, R>> {
        self.bind_associated_function_application_declaration(member, &[])
    }

    pub fn bind_associated_function_application<
        A: KagariArguments + 'static,
        R: KagariType + 'static,
    >(
        &self,
        object_type: &impl InherentType,
        name: &str,
        arguments: &[TypeArgument],
    ) -> NativeResult<PinnedFunction<A, R>> {
        self.bind_associated_function_application_declaration(&object_type.method(name)?, arguments)
    }

    pub fn bind_associated_function_application_declaration<
        A: KagariArguments + 'static,
        R: KagariType + 'static,
    >(
        &self,
        member: &InherentMember,
        arguments: &[TypeArgument],
    ) -> NativeResult<PinnedFunction<A, R>> {
        if member.has_receiver {
            return Err(RuntimeError::module_validation(
                "instance method requires a receiver",
            ));
        }
        self.bind_member_entry(member, arguments)
    }
}

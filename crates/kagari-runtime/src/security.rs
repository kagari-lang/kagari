#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LanguageProfile {
    pub allow_reflection: bool,
    pub allow_reflection_write: bool,
    pub allow_interface_values: bool,
    pub allow_host_calls: bool,
    pub allow_path_mutation: bool,
    pub allow_module_loading: bool,
    pub allow_jit: bool,
    pub allow_eval: bool,
    pub allow_async: bool,
}

impl Default for LanguageProfile {
    fn default() -> Self {
        Self {
            allow_reflection: false,
            allow_reflection_write: false,
            allow_interface_values: true,
            allow_host_calls: false,
            allow_path_mutation: false,
            allow_module_loading: false,
            allow_jit: false,
            allow_eval: false,
            allow_async: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CapabilitySet {
    pub fs_read: bool,
    pub fs_write: bool,
    pub net: bool,
    pub clock: bool,
    pub random: bool,
    pub host_calls: bool,
    pub path_mutation: bool,
    pub reflection_read: bool,
    pub reflection_write: bool,
    pub module_loading: bool,
    pub jit: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SecurityContext {
    pub profile: LanguageProfile,
    pub capabilities: CapabilitySet,
}

impl SecurityContext {
    pub fn allows_reflection_read(&self) -> bool {
        self.profile.allow_reflection && self.capabilities.reflection_read
    }

    pub fn allows_reflection_write(&self) -> bool {
        self.profile.allow_reflection
            && self.profile.allow_reflection_write
            && self.capabilities.reflection_write
    }

    pub fn allows_host_calls(&self) -> bool {
        self.profile.allow_host_calls && self.capabilities.host_calls
    }

    pub fn allows_path_mutation(&self) -> bool {
        self.profile.allow_path_mutation && self.capabilities.path_mutation
    }

    pub fn allows_module_loading(&self) -> bool {
        self.profile.allow_module_loading && self.capabilities.module_loading
    }

    pub fn allows_jit(&self) -> bool {
        self.profile.allow_jit && self.capabilities.jit
    }
}

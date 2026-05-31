#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LanguageProfile {
    pub allow_reflection: bool,
    pub allow_reflection_write: bool,
    pub allow_interface_values: bool,
    pub allow_dynamic_load: bool,
    pub allow_eval: bool,
    pub allow_async: bool,
}

impl Default for LanguageProfile {
    fn default() -> Self {
        Self {
            allow_reflection: false,
            allow_reflection_write: false,
            allow_interface_values: true,
            allow_dynamic_load: false,
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
    pub reflection_read: bool,
    pub reflection_write: bool,
    pub dynamic_load: bool,
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
        self.profile.allow_reflection_write && self.capabilities.reflection_write
    }

    pub fn allows_dynamic_load(&self) -> bool {
        self.profile.allow_dynamic_load && self.capabilities.dynamic_load
    }
}

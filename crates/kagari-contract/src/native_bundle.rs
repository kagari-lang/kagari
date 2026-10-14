//! Bounded, address-free metadata for the first scalar Windows deployment format.
use crate::{
    decode_limits,
    ids::FunctionRef,
    native::{
        ExecutableFunctionArtifact, ExecutableSafepoint, ExecutableSafepointKind,
        ExecutableStackMap,
    },
};
use bincode::{DefaultOptions, Options};
use kagari_abi::{
    native::{BackendId, BackendTarget, ExecutableEntryPoint, ExecutableTrap},
    native_call::JIT_POLL_EXECUTION_SYMBOL,
    version::{KAGARI_RUNTIME_ABI_VERSION, KAGARI_RUNTIME_HELPER_ABI_VERSION},
};
use serde::{Deserialize, Serialize};

pub const TARGET: &str = "x86_64-pc-windows-msvc";
pub const MAX_MANIFEST_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_IMAGE_BYTES: u64 = 64 * 1024 * 1024;
pub const MANIFEST_FILE: &str = "native.kgm";
pub const BYTECODE_FILE: &str = "program.kbc";
pub const IMAGE_FILE: &str = "program.dll";

/// One selected main/public entry. Module/function numbers are file-local slots,
/// resolved against the exact digest-bound verified bytecode Program.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativeBundleManifest {
    pub magic: [u8; 4],
    pub format: u16,
    pub build_contract: String,
    pub runtime_abi: String,
    pub helper_abi: String,
    pub backend: String,
    pub target: String,
    pub pointer_width: u8,
    #[serde(deserialize_with = "decode_limits::nested")]
    pub cpu_features: Vec<String>,
    #[serde(deserialize_with = "decode_limits::nested")]
    pub backend_options: Vec<(String, String)>,
    pub bytecode_digest: [u8; 32],
    pub image_digest: [u8; 32],
    /// None selects the separate main capability; Some selects a public path.
    pub export: Option<String>,
    pub module: u32,
    pub function: u32,
    pub symbol: String,
    #[serde(deserialize_with = "decode_limits::instructions")]
    pub safepoints: Vec<u32>,
    #[serde(deserialize_with = "decode_limits::instructions")]
    pub overflow_points: Vec<u32>,
}

fn codec() -> impl Options {
    DefaultOptions::new()
        .with_fixint_encoding()
        .with_little_endian()
        .reject_trailing_bytes()
        .with_limit(MAX_MANIFEST_BYTES)
}

impl NativeBundleManifest {
    /// Decode with byte/count limits and the host's expected build identity; no code is loaded.
    pub fn decode(bytes: &[u8], expected_build: &str) -> Result<Self, String> {
        if bytes.len() as u64 > MAX_MANIFEST_BYTES {
            return Err("native manifest too large".into());
        }
        let manifest: Self = codec().deserialize(bytes).map_err(|e| e.to_string())?;
        manifest.validate()?;
        if manifest.build_contract != expected_build {
            return Err("incompatible native build contract".into());
        }
        Ok(manifest)
    }

    pub fn encode(&self) -> Result<Vec<u8>, String> {
        self.validate()?;
        codec().serialize(self).map_err(|e| e.to_string())
    }

    /// Check the deployment contract independently of runtime installation.
    /// The only CPU contract accepted in this version is x86_64 baseline SSE2.
    pub fn validate(&self) -> Result<(), String> {
        if self.magic != *b"KGAO" || self.format != 1 || self.build_contract.len() != 64 {
            return Err("incompatible native format/build contract".into());
        }
        if self.runtime_abi != KAGARI_RUNTIME_ABI_VERSION
            || self.helper_abi != KAGARI_RUNTIME_HELPER_ABI_VERSION
        {
            return Err("incompatible native ABI".into());
        }
        if self.backend != "cranelift"
            || self.target != TARGET
            || self.pointer_width != 64
            || self.cpu_features != ["sse2"]
        {
            return Err("unsupported native backend/target/CPU contract".into());
        }
        if self.module as usize >= decode_limits::MAX_MODULES
            || self.function as usize >= decode_limits::MAX_FUNCTIONS
            || self
                .export
                .as_ref()
                .is_some_and(|path| path.is_empty() || path.len() > 4096)
            || self.symbol != format!("kagari_native_{}", self.function)
            || self.backend_options.is_empty()
            || self.backend_options.len() > 64
            || self
                .backend_options
                .iter()
                .any(|(key, value)| key.is_empty() || key.len() > 256 || value.len() > 16384)
            || self.safepoints.is_empty()
            || self.safepoints.len() > decode_limits::MAX_INSTRUCTIONS
            || self.overflow_points.len() > self.safepoints.len()
            || self
                .safepoints
                .iter()
                .enumerate()
                .any(|(index, point)| *point as usize != index)
            || self
                .overflow_points
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
            || self
                .overflow_points
                .iter()
                .any(|point| *point as usize >= self.safepoints.len())
        {
            return Err("invalid native entry/metadata bounds".into());
        }
        for (index, (key, _)) in self.backend_options.iter().enumerate() {
            if self.backend_options[..index]
                .iter()
                .any(|(other, _)| key == other)
            {
                return Err("duplicate backend option".into());
            }
        }
        Ok(())
    }

    /// Resolve symbolic metadata after the caller has established trusted image lifetime.
    pub fn artifact(&self, address: usize) -> ExecutableFunctionArtifact {
        let mut target = BackendTarget::new(&self.target, self.pointer_width);
        target.features = self.cpu_features.clone();
        let mut artifact = ExecutableFunctionArtifact::new(
            BackendId::new(&self.backend),
            target,
            FunctionRef::new(self.function as usize),
        );
        artifact.code.entry = ExecutableEntryPoint::Native {
            symbol: self.symbol.clone(),
            address,
        };
        artifact.safepoints = self
            .safepoints
            .iter()
            .map(|offset| ExecutableSafepoint {
                instruction_offset: *offset as usize,
                kind: ExecutableSafepointKind::RuntimeHelperCall {
                    helper: JIT_POLL_EXECUTION_SYMBOL.into(),
                },
                stack_map: ExecutableStackMap::empty(),
            })
            .collect();
        artifact.code.traps = self
            .overflow_points
            .iter()
            .map(|offset| ExecutableTrap {
                instruction_offset: *offset as usize,
                reason: "integer overflow".into(),
            })
            .collect();
        artifact
    }
}

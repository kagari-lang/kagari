# Cranelift AOT deployment

AOT compiles checked Kagari code on a build machine and deploys a native library
alongside its bytecode Program. JIT and AOT use the same Cranelift scalar lowering
and runtime invocation services. AOT moves compilation out of the serving process;
it does not imply faster execution than JIT or provide wider language coverage.

## Supported deployment

The current target is x86_64 Windows MSVC with baseline SSE2, independent of the
build machine's optional CPU features. It supports the same zero-argument,
single-return-block Unit/Bool/i32 functions as [baseline JIT](jit.md). The SDK
selects one main/public entry per bundle; the CLI selects the separate main entry.
An unsupported entry fails the AOT build before publication. Calls, loops, locals,
reference-bearing values, other numeric families, suspension, Linux AOT and
cross-compilation remain outside this implementation.

A build machine needs the Rust toolchain for building Kagari and an MSVC-compatible
linker for producing the DLL. CLI linking defaults to `lld-link`;
`KAGARI_AOT_LINKER` selects an explicit executable. Generated DLLs use `/NOENTRY`
and `/NODEFAULTLIB`; no Kagari runtime is linked into each script. The server needs
its Kagari host executable and the bundle, without Cranelift, LLVM, linker tools,
MIR or the source frontend. The first implementation does not produce standalone
script executables or general packages.

## Build and load interfaces

SDK features are independent:

| Feature | Capability |
| --- | --- |
| `native` | Existing verified-MIR JIT preparation through a supplied backend |
| `aot-build` | Verified-MIR object preparation and bundle publication; enables `native` and `aot-load` |
| `aot-load` | Trusted bundle loading and installation without compiler/MIR/source dependencies |

Concrete backends remain host-supplied compilation dependencies. Disable SDK
defaults for a compiler-free host. `AotBuild::compile` verifies bytecode/MIR
correspondence, resolves `ProgramEntry` from checked exports and emits through
`ObjectBackend`. `CraneliftBackend::for_aot` selects the baseline target;
`for_host` remains the native-CPU JIT option. AOT builds reject host-tuned
configurations; the current bundle format admits no optional CPU extensions. `NativeObjectProduct` contains object
bytes and symbolic executable metadata, separate from live page-owning products.

`AotBuild::publish` runs a caller-supplied linker callback in a fresh sibling
staging directory. Only successful linking and complete manifest/bytecode writes
permit directory publication. An existing destination is an error, and failures
clean up the owned staging directory. The CLI owns process execution/diagnostics;
the SDK does not assume a linker executable or compile on load.

`unsafe AotBundle::load` checks the bundle and opens the trusted DLL. The bundle
retains a `PreparedProgram` and an executable owner. The host loads that Program
using its ordinary runtime configuration, calls `bundle.install`, then uses
`execute_prepared` and `bundle.entry`. Installation rejects a different prepared
Program, runtime or entry. Host registrations are still explicit and checked.

`JitPolicy` remains the compilation-scheduling option; it does not disable explicit
execution of installed AOT code. `execute_prepared` does not compile. Existing
session cancellation and execution observers apply, and an attached observer
selects reported interpreter execution before native entry. Invalid bundles,
linker/loader errors and failures after native entry are errors, never permission
to silently retry in the interpreter.

## Bundle and trust contract

The bundle contains `program.kbc`, `program.dll` and `native.kgm`; any linker import
library is a build byproduct, not a runtime requirement. Deployment bytecode has no
portable MIR payload. It retains the complete dependency Program and diagnostic
origins; source text is unnecessary for execution.

The bounded manifest format includes format/build identifiers, runtime/helper ABI,
backend configuration, target/pointer width/CPU requirements, BLAKE3 digests of
the exact bytecode and image, the selected main/public path, file-local module and
function slots, the exported symbol, scalar safepoints and overflow origins.
Slots are resolved against verified bytecode, never process-local definition IDs.
Manifest, image and bytecode reads have byte limits; collection counts are bounded
before decoding elements. Unknown formats/contracts, malformed metadata, mismatched
digests/entries and unsupported CPU/target requirements are rejected before loading.
The OS loader resolves the DLL; Kagari does not implement object relocation.

Native code is trusted host code. Metadata checks and hashes prove correspondence,
not publisher authenticity or machine-code correctness. The unsafe loader caller
must guarantee correct generated code, compatible dependencies, no unwinding
through C boundaries and an immutable trusted deployment directory, including
path resolution, from validation through the last retained use. A directory
must not be writable by actors allowed to replace executable inputs. The OS may
execute library initialization before symbol resolution, so arbitrary uploaded
DLLs must not be passed to this API as if it were a bytecode verifier.

Loading uses an absolute canonical path and Windows DLL-directory/System32 search
flags, excluding current-directory discovery. Each published generation uses a
new immutable path. Installed entries and active invocations retain the library
and exact Program/dependency generations. Backend/preparation destruction cannot
retire active code; new publication cannot retarget old entries. The final owner
releases the library, potentially on another host thread. There is no global
native image cache or mutable DLL-global runtime binding.

The physical [native ABI](codegen-backend.md#links-and-runtime-abi) passes a
per-invocation context containing an opaque runtime pointer and immutable helper
table. JIT and AOT call through this table; persistent output contains no build
process address. Scalar polling, checked arithmetic, trace origins and frame/root
cleanup remain owned by the existing runtime. This context does not define the
future general script-call or continuation model.

## Unpublished build compatibility

ABI/format identifiers remain present without a compatibility-version bump for
this internal migration. A conservative build-contract fingerprint hashes normalized
source contents and Cargo manifests for the physical/executable/runtime/compiler/SDK
closure, the SDK fingerprint build script and the workspace lockfile. Build and load
must use the same fingerprint. Changes invalidate development bundles instead of
supporting alternate readers. Comments/test-source edits in that closure can also
invalidate products; this is deliberately conservative, not a compilation cache.

Fingerprint generation runs only with `aot-load` enabled; ordinary runtime and
contract builds do not read compiler sources. The SDK AOT build script currently
requires the repository checkout when building Rust AOT hosts, including compiler source files as hash inputs; it does not compile or
link those compiler crates into a loader-only host. Deployed binaries need no
checkout. Registry publication or distributing a smaller Rust source SDK requires
an explicit replacement for this unpublished-workspace fingerprint input policy.

## Example and verification

From the repository on Windows PowerShell:

```powershell
cargo build --locked -p kagari-cli --features aot
$env:KAGARI_AOT_LINKER = 'C:\Program Files\LLVM\bin\lld-link.exe'
# answer.kgr contains: fn main() -> i32 { 6 * 7 }
.\target\debug\kg.exe build-aot answer.kgr -o release-answer
.\target\debug\kg.exe run-aot release-answer
```

`run-aot` is explicitly a trusted-native-code command and reports the execution
route and returned value. The [compiler-free host example](../../crates/kagari/examples/aot_host.rs)
loads the same bundle with SDK defaults disabled and `aot-load` enabled.
It explicitly installs the CLI producer's `host::log` contract: even an unused
native declaration must match an installed binding before Program publication.

`uv run python scripts/check_aot.py` builds a standalone loader consumer, verifies
its normal/build dependency graph excludes frontend/compiler/Cranelift crates,
builds the bundle in a separate producer, removes script source, and launches the
consumer with only System32 in its tool search path. It asserts Native and 42.
Temporary outputs stay under ignored `target/`; this script is also a Windows CI
acceptance job. Deployment-specific tests live beside the SDK loader; shared
scalar and JIT tests remain at their existing owners. Full workspace, feature and
platform acceptance remains CI work, not an implication of this focused proof.

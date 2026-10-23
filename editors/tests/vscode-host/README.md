# Isolated VS Code host acceptance

Run from the repository root after installing the parent editor dependencies and
packaging `kagari-language` into `target/editors/`:

```sh
node editors/scripts/test-vscode-host.mjs dev --timeout-ms=90000
node editors/scripts/test-vscode-host.mjs installed --timeout-ms=90000
```

The default mode is `both`, run sequentially. Windows discovers the existing
`Code.exe` through `where.exe code`; no editor is downloaded. Other installations
can pass `--vscode=/absolute/path/to/the/editor`. `--vsix=/absolute/package.vsix`
selects an existing local Kagari VSIX. Host timeouts accept 1000..300000 ms
(default 120000); discovery, CLI operations and harness packaging are also bounded.
From `editors/`, `npm run test:vscode-host -- both` exposes the same runner.

Each mode creates a fresh ignored `target/editors/vscode-host-<mode>-*/` directory
for settings, shared storage, extensions, a standalone `.kgr`, launch arguments,
logs and the assertion report. The runner clears inherited VS Code profile/IPC
settings and explicitly sets user-data, extensions, shared-data and portable roots.
In Code 1.142.0, `--user-data-dir` alone does not isolate the new `.vscode-shared`
store: `--shared-data-dir` is required for a normal installed-mode window.
A timeout terminates only its spawned PID tree (Windows) or dedicated process group
(POSIX), never every process with the Code image name. Avoid concurrent runs with
the same installed editor: this Windows build has an application mutex.

## Two execution paths, one suite

- `dev`: `@vscode/test-electron` launches the existing executable with the Kagari
  development path and `index.cjs` as its exported `run()` test entrypoint.
- `installed`: the CLI installs the local Kagari VSIX into the fresh extensions
  directory. Code 1.142.0 does not execute `--extensionTestsPath` by itself, so a
  disposable test-only harness VSIX is also built and CLI-installed there. Its
  startup activation invokes the same `run()` and quits only its isolated editor.
  The normal editor launch has **no** `--extensionDevelopmentPath` or
  `--extensionTestsPath`. The harness asserts its Production `ExtensionContext`;
  the suite separately asserts Kagari's resolved installed path differs from the
  source path. The public `Extension` API has no `extensionMode` property.

The test-only harness's package, runtime and license bundle are staging artifacts,
not part of the declarative Kagari extension or its VSIX. In an accidental install
without the expected isolated environment, the harness refuses to run or quit.
A successful process exit alone is insufficient: the orchestrator also requires
successful assertion reports, rejects failure reports and verifies load origin.

## What the assertions establish

The suite uses native assertions and the actual host's `require('vscode')`. It
opens a standalone file without forcing a language ID and checks `.kgr` recognition,
the loaded Kagari manifest's lack of activation/runtime/server entrypoints and
dependencies, and that Kagari stays inactive. In both Default Light Modern and
Default Dark Modern, it observes line/block comment toggling, delimiter/quote
insertion, no unsupported angle/single-quote pairing, no bracket autoclose in
string/comment text, code pairing inside an identifier interpolation hole, and a nested bracket
jump. No `kg` or Kagari analyzer is launched by this harness.

This is automated host acceptance, **not visual acceptance**. Public VS Code APIs
do not expose TextMate token scopes. Actual engine scope/category/theme-inheritance
assertions remain in `editors/tests/vscode/`; do not mistake semantic-token APIs
for a TextMate inspector. A human still needs to inspect representative examples
with **Developer: Inspect Editor Tokens and Scopes** in both light/dark themes and
confirm visual rendering and incomplete-source recovery. Screenshots alone are
not scope-boundary evidence.

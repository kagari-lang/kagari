# Isolated VS Code host acceptance

Run from the repository root after `npm ci` in `editors/`, building the analyzer
and packaging the VSIX with `npm run package:vscode`:

```sh
node editors/scripts/test-vscode-host.mjs dev --analyzer=/absolute/kagari-analyzer
node editors/scripts/test-vscode-host.mjs installed --analyzer=/absolute/kagari-analyzer
node editors/scripts/test-vscode-host.mjs dev --case=H01,H04 --analyzer=/absolute/kagari-analyzer
node editors/scripts/test-vscode-host.mjs both --suite=grammar
```

The default mode is `both`, sequentially. The client suite defaults to H01–H04
in development mode and H05 in installed mode. `--case` selects a nonempty subset;
incompatible or duplicate cases fail before editor startup. In `both` mode, an
explicit selection must include a case for each mode.
Reports list selected, executed, failed and unrun cases. `--analyzer` accepts only
an absolute existing executable, or use `KAGARI_TEST_ANALYZER`. No analyzer or
editor is downloaded. Windows discovers the installed Code executable; other
installations pass `--vscode=/absolute/editor`. `--vsix=/absolute/package.vsix`
selects a previously packaged VSIX. `--timeout-ms` bounds the host at 1–300 seconds
(default 60 seconds); setup, packaging and CLI operations have separate bounds.
Each case has a 10-second deadline except H04's 30 seconds. H04 deliberately
executes all three recovery delays (250 ms, 1 s and 4 s), waits for cleanup after
each process exit, and verifies manual restart; its observed execution exceeds
10 seconds. This exception changes the test deadline, not a latency target.

| Case | Actual host contract |
| --- | --- |
| H01 | Kagari activation, plaintext exclusion and editing with the server disabled |
| H02 | Registered hover, definition, implementation and completion providers; unsaved diagnostics and repair |
| H03 | Cross-file navigation and readable native/generated definition destinations |
| H04 | Kill the exact owned analyzer PID; bounded retries restore unsaved buffers; retry ceiling, manual restart and disable cleanup |
| H05 | A CLI-installed VSIX loads its bundled client without development paths or installed `node_modules`, and performs a real hover |

The grammar suite separately checks actual editing operations in light and dark
themes with semantic services disabled. Native TextMate/Oniguruma tests own exact
scope and token-category assertions; public VS Code APIs do not expose TextMate
scope inspection. These suites make no claim about every theme's visual appearance.

Each run creates fresh fixtures, settings, extensions, shared and portable storage
under ignored `target/editors/vscode-host-<mode>-*/`. Semantic cases copy the shared
analyzer editor fixture into Unicode/space/`#` paths; buffers and mutations never
touch maintained sources. Setup, install, host and per-case timings are separate.
Event subscriptions precede the tested action, and provider/diagnostic/status
assertions are covered by the case deadline. Individual event waits additionally
expire after 20 seconds. A failed case records its timeout and preserves remaining
cases as unrun; cleanup runs before the host exits. The outer process watchdog
still kills its owned tree if a pending editor operation or cleanup cannot finish.
A successful process exit alone cannot pass.

Outside host time, editor version discovery is bounded at 15 seconds, extension
listing at 30 seconds, and each VSIX packaging/installation operation at 60 seconds.
The Windows watchdog's exact-PID tree termination has a separate 5-second bound.

Development mode uses `@vscode/test-electron` with the existing executable and
the extension's source path. Installed mode CLI-installs the real Kagari VSIX and
a disposable test harness VSIX into the isolated extension directory. Its normal
editor launch has no development/test-path overrides. The harness verifies its
Production extension context; the suite verifies Kagari's resolved installed path
and bundle. The harness refuses to run or quit outside its expected isolated run.

Cleanup stops the client and only the processes owned by the run. A timeout kills
that PID tree on Windows or its dedicated process group on POSIX, never every Code
process. Avoid concurrent host runs against one Windows editor installation because
its application mutex can route launches to an existing window. `--shared-data-dir`
is required in addition to `--user-data-dir` to isolate the shared Code store.

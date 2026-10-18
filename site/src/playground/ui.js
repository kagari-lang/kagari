/** DOM presentation only. The raw client owns Worker admission, generations and quotas. */
export function mountPlayground(root) {
  if (root.dataset.mounted) return;
  root.dataset.mounted = 'true';
  const find = (selector) => root.querySelector(selector);
  const editor = find('[data-source]');
  const runButton = find('[data-action="run"]');
  const cancelButton = find('[data-action="cancel"]');
  const stopButton = find('[data-action="hard-stop"]');
  const resetButton = find('[data-action="reset"]');
  const status = find('[data-status]');
  const output = find('[data-output]');
  const result = find('[data-result]');
  const diagnostics = find('[data-diagnostics]');
  const sourceLimit = Number(root.dataset.sourceLimit);
  const encoder = new TextEncoder();
  let client;
  let phase = 'idle';
  let operation = 0;
  let submittedSource = '';
  let oversized = false;

  function setPhase(next) {
    phase = next;
    root.dataset.state = next;
    runButton.disabled = next !== 'idle' || oversized;
    cancelButton.disabled = !['loading', 'running'].includes(next);
    stopButton.disabled = next === 'idle' && !client;
    resetButton.disabled = next === 'resetting';
  }

  function updateSource() {
    // Avoid encoding an already oversized paste just to count its bytes.
    const bytes = editor.value.length > sourceLimit ? null : encoder.encode(editor.value).length;
    oversized = bytes === null || bytes > sourceLimit;
    find('[data-source-size]').textContent = bytes === null
      ? 'Source exceeds 256 KiB of UTF-8.'
      : `${bytes.toLocaleString()} / ${sourceLimit.toLocaleString()} UTF-8 bytes`;
    find('[data-source-error]').hidden = !oversized;
    editor.setAttribute('aria-invalid', String(oversized));
    const stale = editor.value !== submittedSource;
    const locations = diagnostics.querySelectorAll('[data-diagnostic-location]');
    for (const button of locations) button.disabled = stale;
    find('[data-diagnostics-stale]').hidden = !stale || diagnostics.childElementCount === 0;
    setPhase(phase);
  }

  function clearDisplay() {
    output.textContent = '';
    result.textContent = 'No result yet.';
    diagnostics.replaceChildren();
    find('[data-diagnostics-empty]').hidden = false;
    find('[data-diagnostics-stale]').hidden = true;
    find('[data-output-truncated]').hidden = true;
    find('[data-diagnostics-truncated]').hidden = true;
  }

  function appendText(parent, tag, text) {
    const element = document.createElement(tag);
    element.textContent = text;
    parent.append(element);
    return element;
  }

  function appendLocation(parent, location) {
    if (!location) return;
    const { origin, start, end } = location;
    const label = `${origin}:${start.line + 1}:${start.character + 1}`;
    const selectable = origin === 'memory://main.kgr'
      && start.offset <= end.offset && end.offset <= submittedSource.length;
    if (!selectable) {
      appendText(parent, 'p', label);
      return;
    }
    const button = appendText(parent, 'button', `Select ${label}`);
    button.type = 'button';
    button.dataset.diagnosticLocation = '';
    button.dataset.start = String(start.offset);
    button.dataset.end = String(end.offset);
    button.title = `UTF-16 range [${start.offset}, ${end.offset}); revision ${location.revision}`;
    button.addEventListener('click', () => {
      if (editor.value !== submittedSource) {
        updateSource();
        return;
      }
      editor.focus();
      editor.setSelectionRange(start.offset, end.offset);
    });
  }

  function renderDiagnostics(response) {
    diagnostics.replaceChildren();
    for (const diagnostic of response.diagnostics) {
      const item = document.createElement('li');
      appendText(item, 'p', `${diagnostic.severity} ${diagnostic.code}: ${diagnostic.message}`);
      appendLocation(item, diagnostic.location);
      for (const label of diagnostic.labels) {
        appendText(item, 'p', label.message);
        appendLocation(item, label.location);
      }
      for (const note of diagnostic.notes) appendText(item, 'p', `Note: ${note}`);
      if (diagnostic.truncated) appendText(item, 'p', 'This diagnostic was truncated.');
      diagnostics.append(item);
    }
    find('[data-diagnostics-empty]').hidden = response.diagnostics.length !== 0;
    find('[data-diagnostics-truncated]').hidden = !response.diagnostics_truncated;
    updateSource();
  }

  // Never await run/reset here: the client must ACK this observation before settling them.
  function onEvent(event) {
    if (!client) return;
    const lifecycle = event.sequence === 0;
    const generation = client.workerGeneration - (lifecycle ? 1 : 0);
    if (event.workerGeneration !== generation || event.runGeneration !== client.runGeneration) return;
    root.dataset.workerGeneration = String(event.workerGeneration);
    root.dataset.runGeneration = String(event.runGeneration);
    root.dataset.event = event.type;
    if (lifecycle) {
      if (event.type === 'fatal') {
        result.textContent = event.message;
        status.textContent = 'Fatal Worker or WASM failure. The instance was destroyed; Run will create a fresh one.';
      } else if (event.type === 'stopped') {
        status.textContent = 'Hard stopped. The old instance was destroyed; completed effects were not rolled back.';
      } else if (event.type === 'disposed') {
        status.textContent = 'Instance released. Run will create a fresh one.';
      }
      return;
    }
    const response = event.response;
    // Responses contain only new messages, including the final response's remainder.
    for (const message of response.output) output.append(document.createTextNode(`${message}\n`));
    if (response.output_truncated) find('[data-output-truncated]').hidden = false;
    if (event.type === 'progress') {
      if (phase === 'running') {
        status.textContent = response.status === 'waiting' ? 'Waiting for execution to wake…' : 'Running…';
      }
      return;
    }
    if (event.type === 'reset' && response.status === 'idle') {
      clearDisplay();
      status.textContent = 'Reset complete. Source kept; runtime state and displayed results cleared.';
      return;
    }
    renderDiagnostics(response);
    if (response.error) {
      result.textContent = `${response.error.code}: ${response.error.message}${response.error.truncated ? '\nError text truncated.' : ''}`;
      status.textContent = response.error.code === 'KG_RUNTIME_CANCELLED'
        ? 'Cancelled with normal cleanup. Completed effects are retained.'
        : 'Operation failed. See diagnostics and result.';
    } else {
      // Tagged integer strings stay strings; do not convert i64/u64 through Number.
      result.textContent = response.result === null ? 'No returned value.' : JSON.stringify(response.result, null, 2);
      status.textContent = 'Run complete.';
    }
    if (phase === 'resetting' && event.type === 'result') status.textContent = 'Resetting runtime state…';
  }

  function showFailure(error) {
    result.textContent = String(error?.message ?? error);
    status.textContent = 'Could not complete the request. Check the result and try again.';
  }

  async function run() {
    updateSource();
    if (phase !== 'idle' || oversized) return;
    const current = ++operation;
    clearDisplay();
    // Textarea values normalize CRLF/lone CR to LF. Submit that same snapshot,
    // never an unnormalized backing string: adapter UTF-16 offsets then match
    // setSelectionRange, including astral characters and pasted Windows source.
    editor.value = editor.value.replace(/\r\n?/gu, '\n');
    submittedSource = editor.value;
    setPhase(client ? 'running' : 'loading');
    status.textContent = client ? 'Compiling and running…' : 'Loading compiler client…';
    try {
      if (!client) {
        // Keep deployment assets out of Vite's eager dependency/preload graph.
        const { PlaygroundClient } = await import(/* @vite-ignore */ root.dataset.clientUrl);
        if (current !== operation) return;
        client = new PlaygroundClient({ onEvent });
      }
      setPhase('running');
      status.textContent = 'Compiling and running… Compilation cannot be cooperatively interrupted.';
      await client.run(submittedSource);
    } catch (error) {
      if (current === operation) showFailure(error);
    } finally {
      if (current === operation) setPhase('idle');
    }
  }

  function cancel() {
    if (phase === 'loading') {
      ++operation;
      setPhase('idle');
      status.textContent = 'Cancelled before execution. No Worker was started.';
    } else if (phase === 'running') {
      // Set this before cancel: posting can synchronously report a fatal fault.
      setPhase('cancelling');
      status.textContent = 'Cancellation requested. Waiting for a yield; use Hard stop if compilation or a native call does not return.';
      client.cancel();
    }
  }

  function hardStop() {
    ++operation;
    if (client) client.hardStop();
    else status.textContent = 'Stopped before execution. No Worker was started.';
    setPhase('idle');
  }

  async function reset() {
    if (phase === 'resetting') return;
    const current = ++operation;
    if (!client) {
      clearDisplay();
      setPhase('idle');
      status.textContent = 'Display reset. Source kept; the compiler has not been started.';
      return;
    }
    setPhase('resetting');
    status.textContent = 'Reset requested. Waiting for cooperative cleanup; use Hard stop if it cannot finish.';
    try {
      await client.reset();
    } catch (error) {
      if (current === operation) showFailure(error);
    } finally {
      if (current === operation) setPhase('idle');
    }
  }

  editor.addEventListener('input', updateSource);
  editor.addEventListener('keydown', (event) => {
    if (event.key === 'Enter' && (event.ctrlKey || event.metaKey) && !event.isComposing) {
      event.preventDefault();
      if (!event.repeat) void run();
    }
  });
  runButton.addEventListener('click', () => { void run(); });
  cancelButton.addEventListener('click', cancel);
  stopButton.addEventListener('click', hardStop);
  resetButton.addEventListener('click', () => { void reset(); });
  window.addEventListener('pagehide', () => {
    ++operation;
    client?.dispose();
    client = undefined;
    setPhase('idle');
  });
  updateSource();
}

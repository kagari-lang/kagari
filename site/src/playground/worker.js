import { LIMITS, VERSION, OutputBudget, emptyResponse, parseResponse, validCommand } from './protocol.js';

// One module/instance per Worker; a fresh BrowserSession per source bounds history.
// Only application-owned product URLs are accepted from the client, never from script.
let generation;
let lastRun = 0;
let modulePromise;
let productURL;
let active;
let invalid = false;

function current(run) {
  return !invalid && active === run;
}

function guarded(run, action) {
  if (!current(run)) return;
  try { action(); } catch (error) { fatal(run, error); }
}

function fatal(run, error) {
  if (invalid) return;
  invalid = true;
  clearTimeout(run.driveTimer);
  clearTimeout(run.flushTimer);
  // Do not reset/free/call back into an instance after ANY unexpected exception.
  // One fatal control message may bypass the outstanding response credit.
  self.postMessage({ version: VERSION, type: 'fatal', workerGeneration: generation,
    runGeneration: run.id, sequence: ++run.sequence,
    message: String(error instanceof Error ? error.message : error).slice(0, 1000) });
  self.close();
}

function absorb(run, raw, statuses) {
  const response = parseResponse(raw);
  if (!statuses.includes(response.status)) throw new Error(`Unexpected adapter state: ${response.status}`);
  run.output.append(response);
  run.response = { ...response, output: [] };
  return response;
}

function flush(run) {
  if (!current(run) || run.inFlight || !run.dirty) return;
  clearTimeout(run.flushTimer);
  run.flushTimer = undefined;
  run.dirty = false;
  const type = run.terminal ?? 'progress';
  const event = { version: VERSION, type, workerGeneration: generation,
    runGeneration: run.id, sequence: ++run.sequence, response: run.output.drain(run.response) };
  run.inFlight = event.sequence;
  self.postMessage(event);
}

function publishLater(run) {
  run.dirty = true;
  if (run.inFlight || run.flushTimer !== undefined) return;
  run.flushTimer = setTimeout(() => guarded(run, () => flush(run)), LIMITS.flushMilliseconds);
}

function finish(run, type) {
  clearTimeout(run.driveTimer);
  run.driveTimer = undefined;
  // Ordinary script/compile errors still retire owners through the normal SDK path.
  // Reset failure is visible, not swallowed; it is not a fatal WASM exception.
  const cleanup = parseResponse(run.session.reset());
  if (!['idle', 'error'].includes(cleanup.status)) throw new Error('Unexpected cleanup state');
  run.output.append(cleanup);
  if (type === 'reset' || cleanup.status === 'error') run.response = { ...cleanup, output: [] };
  run.session.free();
  run.session = undefined;
  run.terminal = type;
  run.dirty = true;
  flush(run);
}

function scheduleDrive(run) {
  if (!current(run) || run.terminal || run.driveTimer !== undefined) return;
  // A real task, not a self-replenishing Promise chain: stop messages get a turn.
  run.driveTimer = setTimeout(() => guarded(run, () => {
    run.driveTimer = undefined;
    const before = run.response.status;
    const messages = run.output.messages;
    const truncated = run.output.truncated;
    const response = absorb(run, run.session.drive(run.slice), ['runnable', 'waiting', 'complete', 'error']);
    if (['complete', 'error'].includes(response.status)) { finish(run, 'result'); return; }
    if (messages !== run.output.messages || truncated !== run.output.truncated || before !== response.status) publishLater(run);
    if (response.status === 'runnable') scheduleDrive(run);
    // Waiting parks indefinitely. No external providers/wake callbacks are installed.
    // Cancellation is the only current wake and schedules the same guarded task.
  }), 0);
}

async function start(run, message) {
  try {
    productURL ??= message.productURL;
    if (productURL !== message.productURL) throw new Error('Product URL changed within a Worker');
    modulePromise ??= (async () => {
      const module = await import(productURL);
      await module.default();
      if (typeof module.BrowserSession?.prototype.compile !== 'function') throw new Error('Playground requires the source product');
      return module;
    })();
    const module = await modulePromise;
    if (!current(run)) return;
    run.session = new module.BrowserSession();
    if (run.cancelled) {
      run.response = emptyResponse('error');
      run.response.error = { code: 'KG_RUNTIME_CANCELLED', severity: 'error', message: 'Cancelled before compilation', truncated: false };
      finish(run, 'result');
      return;
    }
    const compiled = absorb(run, run.session.compile(message.source), ['ready', 'error']);
    if (compiled.status === 'error') { finish(run, 'result'); return; }
    const started = absorb(run, run.session.start(), ['runnable', 'error']);
    if (started.status === 'error') { finish(run, 'result'); return; }
    run.dirty = true;
    flush(run);
    scheduleDrive(run);
  } catch (error) { if (current(run)) fatal(run, error); }
}

self.addEventListener('message', ({ data: message }) => {
  // Malformed/stale/busy messages are dropped without replies (no rejection flood).
  if (invalid || !validCommand(message)) return;
  if (generation !== undefined && message.workerGeneration !== generation) return;
  if (message.type === 'run' || message.type === 'reset') {
    if (active || message.runGeneration <= lastRun) return;
    generation ??= message.workerGeneration;
    lastRun = message.runGeneration;
    const run = { id: lastRun, slice: message.slice, sequence: 0, output: new OutputBudget(),
      response: emptyResponse('idle'), cancelled: false };
    active = run;
    if (message.type === 'reset') {
      // No active owner survives a terminal ACK; idle reset needs no WASM init.
      run.terminal = 'reset';
      run.dirty = true;
      guarded(run, () => flush(run));
    } else void start(run, message);
    return;
  }
  const run = active;
  if (!run || message.runGeneration !== run.id) return;
  guarded(run, () => {
    if (message.type === 'ack') {
      if (message.sequence !== run.inFlight) return;
      run.inFlight = undefined;
      if (run.terminal && !run.dirty) { active = undefined; return; }
      if (run.terminal) flush(run);
      else if (run.dirty) publishLater(run);
    } else if (message.type === 'cancel' && !run.cancelled && !run.terminal) {
      run.cancelled = true;
      if (run.session) { run.session.cancel(); scheduleDrive(run); }
    }
  });
});
self.addEventListener('messageerror', () => {
  if (active) fatal(active, new Error('Worker message could not be decoded'));
  else {
    invalid = true;
    // No run identity exists to reply with. Surface a Worker error so the client
    // retires it instead of keeping a silently closed instance for the next run.
    throw new Error('Idle Worker message could not be decoded');
  }
});

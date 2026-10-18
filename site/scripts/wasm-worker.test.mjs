import assert from 'node:assert/strict';
import { test } from 'node:test';
import { PlaygroundClient } from '../src/playground/client.js';
import { LIMITS, VERSION, OutputBudget, byteLength, emptyResponse, parseResponse, validCommand, validEvent, validateSlice, validateSource } from '../src/playground/protocol.js';

const command = (extra = {}) => ({ version: VERSION, workerGeneration: 1, runGeneration: 1, type: 'run', source: 'fn main() {}', slice: 100, productURL: 'http://localhost/source.js', ...extra });
const event = (type = 'result', extra = {}) => ({ version: VERSION, workerGeneration: 1, runGeneration: 1, sequence: 1, type, response: emptyResponse(type === 'progress' ? 'runnable' : 'complete'), ...extra });
const tick = () => new Promise((resolve) => setTimeout(resolve, 0));

class FakeWorker {
  static instances = [];
  listeners = new Map();
  sent = [];
  terminated = false;

  constructor() { FakeWorker.instances.push(this); }
  addEventListener(type, listener) { this.listeners.set(type, listener); }
  postMessage(message) { this.sent.push(message); }
  terminate() { this.terminated = true; }
  emit(data) { this.listeners.get('message')({ data }); }
}

async function withWorkers(action) {
  const previous = globalThis.Worker;
  globalThis.Worker = FakeWorker;
  FakeWorker.instances = [];
  try { await action(); }
  finally { globalThis.Worker = previous; }
}

test('UTF-8 admission and slice validation precede the u32 ABI', () => {
  for (const slice of [0, -1, 0.5, NaN, Infinity, -Infinity, 100001, 2 ** 32 + 1, '1', null, undefined]) {
    assert.throws(() => validateSlice(slice), RangeError);
    assert.equal(validCommand(command({ slice })), false);
  }
  for (const slice of [1, LIMITS.defaultSlice, LIMITS.maxSlice]) assert.equal(validateSlice(slice), slice);
  const boundary = '😀'.repeat(LIMITS.sourceBytes / 4);
  assert.equal(validateSource(boundary), boundary);
  assert.throws(() => validateSource(`${boundary}a`), RangeError);
  assert.throws(() => validateSource('x'.repeat(LIMITS.sourceBytes + 1)), RangeError);
  assert.throws(() => validateSource({}), TypeError);
  for (const value of [null, [], {}, command({ version: 2 }), command({ runGeneration: 0 }), command({ type: 'wake' }), command({ source: 1 })]) assert.equal(validCommand(value), false);
});

test('cumulative output counts UTF-8 across drains and keeps a bounded prefix', () => {
  const budget = new OutputBudget();
  for (let index = 0; index < 300; index += 1) {
    budget.append({ output: ['x'], output_truncated: false });
    const drained = budget.drain(emptyResponse('runnable'));
    assert.equal(drained.output.length, index < LIMITS.outputMessages ? 1 : 0);
  }
  assert.equal(budget.messages, LIMITS.outputMessages);
  assert.equal(budget.truncated, true);
  const bytes = new OutputBudget();
  const message = '😀'.repeat(LIMITS.messageBytes / 4);
  for (let index = 0; index < 20; index += 1) bytes.append({ output: [message], output_truncated: false });
  assert.equal(bytes.bytes, LIMITS.outputBytes);
  assert.equal(bytes.pending.length, LIMITS.outputBytes / byteLength(message));
  assert.equal(bytes.truncated, true);
  const lower = new OutputBudget();
  lower.append({ output: ['prefix'], output_truncated: true });
  lower.append({ output: ['later drain'], output_truncated: false });
  assert.deepEqual(lower.drain(emptyResponse('complete')).output, ['prefix', 'later drain']);
  assert.equal(lower.truncated, true, 'adapter truncation stays visible without exhausting run quota');
});

test('adapter/event validation rejects malformed envelopes and unbounded fields', () => {
  assert.deepEqual(parseResponse(JSON.stringify(emptyResponse('idle'))), emptyResponse('idle'));
  assert.throws(() => parseResponse('{}'));
  assert.throws(() => parseResponse('x'.repeat(LIMITS.responseCharacters + 1)));
  assert.equal(validEvent(event()), true);
  const text = '\0'.repeat(LIMITS.messageBytes);
  const position = { offset: 0, line: 0, character: 0 };
  const location = { origin: text, revision: '1', byte_start: 0, byte_end: 0, start: position, end: position };
  const diagnostic = { code: 'KG_TEST', severity: 'error', message: text, truncated: false, location,
    labels: Array.from({ length: 8 }, () => ({ location, message: text })), notes: Array(8).fill(text) };
  const maximum = JSON.stringify({ ...emptyResponse('error'), diagnostics: Array(32).fill(diagnostic),
    output: Array(16).fill(text), error: { code: 'KG_TEST', severity: 'error', message: text, truncated: false } });
  assert(maximum.length <= LIMITS.responseCharacters, 'raw bound includes maximum diagnostics and JSON escaping');
  assert.equal(parseResponse(maximum).diagnostics.length, 32);
  for (const response of [{}, { ...emptyResponse('complete'), output: ['x'.repeat(LIMITS.messageBytes + 1)] }, { ...emptyResponse('complete'), result: { tag: 'u64', value: 18446744073709551615 } }]) {
    assert.equal(validEvent(event('result', { response })), false);
  }
});

test('client has no caller queue, coalesces cancellation, and ACKs after observer', async () => withWorkers(async () => {
  let release;
  const gate = new Promise((resolve) => { release = resolve; });
  const seen = [];
  const client = new PlaygroundClient({ onEvent: (value) => { seen.push(value); return gate; } });
  assert.throws(() => client.run('😀'.repeat(65537)), RangeError);
  assert.equal(FakeWorker.instances.length, 0, 'oversize admission must not even construct Worker');
  const result = client.run('fn main() {}');
  const worker = FakeWorker.instances[0];
  for (let index = 0; index < 100; index += 1) assert.throws(() => client.run('fn main() {}'), /busy/u);
  assert.equal(worker.sent.length, 1);
  assert.equal(client.cancel(), true);
  assert.equal(client.cancel(), false);
  assert.equal(worker.sent.length, 2);
  worker.emit(event());
  await tick();
  assert.equal(worker.sent.length, 2, 'observer holds response credit');
  release();
  assert.equal((await result).type, 'result');
  assert.equal(worker.sent.at(-1).type, 'ack');
  assert.equal(client.busy, false);
  assert.equal(seen.length, 1);
  client.dispose();
}));

test('reset holds admission across cancellation then idle handshake', async () => withWorkers(async () => {
  const client = new PlaygroundClient();
  const run = client.run('fn main() { loop {} }');
  const worker = FakeWorker.instances[0];
  const reset = client.reset();
  assert.equal(client.reset(), reset);
  assert.throws(() => client.run(''), /busy/u);
  worker.emit(event());
  await run;
  await tick();
  assert.deepEqual(worker.sent.map((item) => item.type), ['run', 'cancel', 'ack', 'reset']);
  worker.emit(event('reset', { runGeneration: 2, response: emptyResponse('idle') }));
  assert.equal((await reset).type, 'reset');
  assert.equal(client.busy, false);
  client.dispose();
}));

test('hard stop invalidates old replies and async observer continuations', async () => withWorkers(async () => {
  let release;
  const gate = new Promise((resolve) => { release = resolve; });
  const seen = [];
  const client = new PlaygroundClient({ onEvent: (value) => { seen.push(value); if (value.type === 'progress') return gate; } });
  const first = client.run('fn main() {}');
  const old = FakeWorker.instances[0];
  old.emit(event('progress'));
  const reset = client.reset();
  client.hardStop();
  assert.equal((await first).type, 'stopped');
  assert.equal((await reset).type, 'stopped');
  assert.equal(old.terminated, true);
  const next = client.run('fn main() {}');
  const fresh = FakeWorker.instances[1];
  old.emit(event('result', { sequence: 2 }));
  fresh.emit(event('result', { workerGeneration: 1, runGeneration: 2 }));
  fresh.emit(event('result', { workerGeneration: 2, runGeneration: 1 }));
  release();
  await tick();
  assert.equal(fresh.sent.length, 1, 'old observer must not ACK/wake the new Worker');
  assert.deepEqual(seen.map((item) => item.type), ['progress', 'stopped']);
  fresh.emit(event('result', { workerGeneration: 2, runGeneration: 2 }));
  assert.equal((await next).workerGeneration, 2);
  assert.equal(client.dispose().type, 'disposed');
  assert.equal(client.dispose(), client.dispose());
  assert.throws(() => client.run(''), /disposed/u);
}));

test('malformed, out-of-credit and fatal responses retire instances; next run recovers', async () => withWorkers(async () => {
  const client = new PlaygroundClient();
  const first = client.run('');
  FakeWorker.instances[0].emit(event('result', { response: {} }));
  assert.equal((await first).type, 'fatal');
  assert.equal(FakeWorker.instances[0].terminated, true);
  const second = client.run('');
  FakeWorker.instances[1].emit(event('result', { workerGeneration: 2, runGeneration: 2, sequence: 2 }));
  assert.equal((await second).type, 'fatal');
  const third = client.run('');
  FakeWorker.instances[2].emit(event('fatal', { workerGeneration: 3, runGeneration: 3, message: 'controlled failure' }));
  assert.equal((await third).type, 'fatal');
  const recovered = client.run('');
  FakeWorker.instances[3].emit(event('result', { workerGeneration: 4, runGeneration: 4 }));
  assert.equal((await recovered).type, 'result');
  client.dispose();
}));

test('observer failure retires rather than leaving a response-credit deadlock', async () => withWorkers(async () => {
  const client = new PlaygroundClient({ onEvent: () => { throw new Error('observer'); } });
  const result = client.run('');
  FakeWorker.instances[0].emit(event());
  assert.equal((await result).type, 'fatal');
  assert.equal(FakeWorker.instances[0].terminated, true);
  client.dispose();
}));

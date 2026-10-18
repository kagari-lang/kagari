export const workerCases = [
  'cooperative-loop-reset-effects',
  'diagnostics-lossless-results-trap-order',
  'cumulative-output-message-and-byte-bounds',
  'response-backpressure-cancellation',
  'worker-admission-stale-generations',
  'client-drops-retired-and-duplicate-replies',
  'waiting-parks-and-cancel-wakes',
  'hard-stop-nonyielding-compile',
  'fatal-drive-fresh-instance',
  'malformed-adapter-fresh-instance',
];

// Serialized into a real Chrome page; production modules/Worker/WASM are fetched
// from the self-owned immutable server. No production test hooks or page-side WASM.
export async function workerCase(name) {
  const { PlaygroundClient } = await import('/kagari/wasm/playground/client.js');
  const { LIMITS, VERSION, byteLength } = await import('/kagari/wasm/playground/protocol.js');
  const equal = (actual, expected, label) => {
    if (JSON.stringify(actual) !== JSON.stringify(expected)) throw new Error(`${label}: ${JSON.stringify(actual)} != ${JSON.stringify(expected)}`);
  };
  const check = (condition, label) => { if (!condition) throw new Error(label); };
  const delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const until = async (predicate, label) => {
    const deadline = performance.now() + 12_000;
    while (!predicate()) {
      if (performance.now() > deadline) throw new Error(`Timed out: ${label}`);
      await delay(10);
    }
  };
  const clients = [];
  const workers = [];
  const events = [];
  window.wb04Trace = [];
  const trace = (event) => {
    if (window.wb04Trace.length < 80) window.wb04Trace.push({ type: event.type, worker: event.workerGeneration, run: event.runGeneration, sequence: event.sequence,
      status: event.response?.status, outputCount: event.response?.output.length, truncated: event.response?.output_truncated, error: event.response?.error?.code, message: event.message });
  };
  const make = (options = {}) => {
    const client = new PlaygroundClient({ onEvent: (event) => { events.push(event); trace(event); }, ...options });
    clients.push(client);
    return client;
  };
  const output = (selected = events) => selected.flatMap((event) => event.response?.output ?? []);
  const assertResult = (event, value = '42') => {
    equal(event.type, 'result', 'terminal kind');
    equal(event.response.status, 'complete', 'successful execution');
    equal(event.response.result, { tag: 'i32', value }, 'owned result');
  };
  try {
    if (name === 'cooperative-loop-reset-effects') {
      const client = make({ slice: 100 });
      const source = 'use web::print; fn main() { val retained = ["rooted 😀"]; val values = Vec::from([1, 2]); for value in values { print(retained[0]); loop {} } }';
      let ticks = 0;
      const timer = setInterval(() => { ticks += 1; }, 5);
      try {
        const run = client.run(source);
        let busy = 0;
        for (let index = 0; index < 100; index += 1) {
          try { client.run('fn main() {}'); } catch (error) { if (/busy/u.test(error.message)) busy += 1; }
        }
        equal(busy, 100, 'bounded admission before posting');
        await until(() => output().includes('rooted 😀'), 'loop reaches retained root and collection cursor');
        await delay(60);
        check(ticks >= 3, 'main event loop remains responsive');
        check(client.cancel(), 'first cancel accepted');
        check(!client.cancel(), 'duplicate cancel coalesced');
        const cancelled = await run;
        equal(cancelled.response.error.code, 'KG_RUNTIME_CANCELLED', 'driver terminal cancellation');
        equal(output(), ['rooted 😀'], 'completed effects exactly once');
        const generation = client.workerGeneration;
        for (let index = 0; index < 3; index += 1) assertResult(await client.run('fn main() -> i32 { val values = Vec::from([40, 2]); values[0] + values[1] }'));
        equal(client.workerGeneration, generation, 'normal cancellation reuses healthy WASM instance');
        const active = client.run(source);
        await until(() => output().length === 2, 'second live loop');
        const reset = client.reset();
        equal(client.reset() === reset, true, 'reset coalesced');
        equal((await active).response.error.code, 'KG_RUNTIME_CANCELLED', 'reset drives cancellation to terminal');
        equal((await reset).response.status, 'idle', 'reset acknowledgement');
        assertResult(await client.run('fn main() -> i32 { 42 }'));
        return { mainTaskTicks: ticks, busyRejected: busy, cancellation: cancelled.response.error.code, sameWorkerRecovery: true,
          boundary: 'Live retained array/String and collection cursor cancelled through real SDK; exact root/loan counts are not exported by BrowserSession.' };
      } finally { clearInterval(timer); }
    }
    if (name === 'diagnostics-lossless-results-trap-order') {
      const client = make({ slice: 100 });
      const diagnostic = await client.run('// 😀\r\nfn main() -> i32 { missing }');
      equal(diagnostic.response.status, 'error', 'compile error');
      const location = diagnostic.response.diagnostics[0].location;
      equal(location.origin, 'memory://main.kgr', 'diagnostic origin');
      equal(location.byte_start, 28, 'UTF-8 span');
      equal(location.start, { offset: 26, line: 1, character: 19 }, 'UTF-16 CRLF position');
      const wide = await client.run('fn main() -> u64 { 18446744073709551615u64 }');
      equal(wide.response.result, { tag: 'u64', value: '18446744073709551615' }, 'lossless integer');
      const trapped = await client.run('use web::print; fn base(values: [i32]) -> [i32] { print("base"); values } fn index() -> u64 { print("index"); 18446744073709551615u64 } fn replacement() -> i32 { print("replacement"); 9 } fn main() { val values = [7]; base(values)[index()] = replacement(); print("after"); }');
      equal(trapped.response.status, 'error', 'checked trap');
      equal(output(), ['base', 'index', 'replacement'], 'left-to-right once-only effects before trap');
      assertResult(await client.run('fn main() -> i32 { 42 }'));
      return { location, wide: wide.response.result, trap: trapped.response.error, output: output() };
    }
    if (name === 'cumulative-output-message-and-byte-bounds') {
      const client = make({ slice: 100 });
      const count = await client.run('use web::print; fn main() { for i in 0..300 { print("x"); } }');
      equal(count.response.status, 'complete', 'count workload completes');
      equal(output().length, LIMITS.outputMessages, 'cumulative messages across small slices');
      check(output().every((text) => text === 'x'), 'count output contents');
      equal(count.response.output_truncated, true, 'visible count truncation');
      const countEvents = events.length;
      events.length = 0;
      const text = '😀'.repeat(1025);
      const bytes = await client.run(`use web::print; fn main() { for i in 0..20 { print("${text}"); } }`);
      equal(bytes.response.status, 'complete', 'byte workload completes');
      equal(output().reduce((sum, item) => sum + byteLength(item), 0), LIMITS.outputBytes, 'cumulative UTF-8 bytes');
      equal(output().length, 16, 'byte-limited message count');
      check(output().every((item) => item === '😀'.repeat(1024)), 'UTF-8 codepoints preserved');
      equal(bytes.response.output_truncated, true, 'visible byte truncation');
      events.length = 0;
      await client.run('use web::print; fn main() { print("fresh"); }');
      equal(output(), ['fresh'], 'quota resets for next run');
      let rejected = false;
      try { client.run('😀'.repeat(65537)); } catch (error) { rejected = error instanceof RangeError; }
      check(rejected, 'UTF-8 oversize admission');
      return { cumulativeMessages: LIMITS.outputMessages, cumulativeBytes: LIMITS.outputBytes, countEvents, sourceCap: LIMITS.sourceBytes };
    }
    if (name === 'response-backpressure-cancellation') {
      let release;
      const gate = new Promise((resolve) => { release = resolve; });
      const client = make({ slice: 1000, onEvent: (event) => {
        events.push(event); trace(event);
        if (event.type === 'progress') return gate;
      } });
      const run = client.run('use web::print; fn main() { loop { print("effect"); } }');
      await until(() => events.length === 1, 'first response holds credit');
      await delay(350);
      equal(events.length, 1, 'no flood while response consumer is blocked');
      client.cancel();
      await delay(100);
      equal(events.length, 1, 'terminal also waits for response credit');
      release();
      const terminal = await run;
      equal(terminal.response.error.code, 'KG_RUNTIME_CANCELLED', 'cleanup ran despite blocked observer');
      check(output().length > 0 && output().length <= LIMITS.outputMessages, 'execution proceeded under bounded output');
      equal(terminal.response.output_truncated, true, 'flood is visibly truncated');
      equal(events.length, 2, 'one progress and one terminal only');
      assertResult(await client.run('fn main() -> i32 { 42 }'));
      // With credit flowing, saturated output must also become quiet instead of
      // emitting empty per-slice/per-timer progress forever.
      events.length = 0;
      const flood = client.run('use web::print; fn main() { loop { print("effect"); } }');
      await until(() => events.some((event) => event.response?.output_truncated), 'run quota reached');
      const saturated = events.length;
      await delay(150);
      equal(events.length, saturated, 'no redundant progress after saturation');
      client.cancel();
      equal((await flood).response.error.code, 'KG_RUNTIME_CANCELLED', 'saturated run remains cancellable');
      return { heldResponseMilliseconds: 450, responsesBeforeRelease: 1, saturatedResponses: saturated, outputMessages: output().length, cancellation: terminal.response.error.code };
    }
    if (name === 'worker-admission-stale-generations') {
      const worker = new Worker('/kagari/wasm/playground/worker.js', { type: 'module' });
      workers.push(worker);
      worker.onmessage = ({ data }) => { events.push(data); trace(data); };
      const message = (type, extra = {}) => ({ version: VERSION, workerGeneration: 7, runGeneration: 11, type, ...extra });
      const run = message('run', { source: 'fn main() { loop {} }', slice: 100, productURL: new URL('/kagari/wasm/source/kagari_web.js', location.href).href });
      for (const malformed of [null, {}, { ...run, version: 99 }, ...[NaN, Infinity, 0, -1, 1.5, 100001, 4294967297].map((slice) => ({ ...run, slice })), { ...run, source: '😀'.repeat(65537) }]) worker.postMessage(malformed);
      worker.postMessage(run);
      await until(() => events.length === 1, 'valid run after malformed traffic');
      equal(events[0].response.status, 'runnable', 'invalid messages did not create a run');
      worker.postMessage({ ...run, runGeneration: 12, source: 'fn main() -> i32 { 99 }' });
      worker.postMessage(message('cancel', { workerGeneration: 6 }));
      worker.postMessage(message('cancel', { runGeneration: 10 }));
      worker.postMessage(message('ack', { sequence: 999 }));
      worker.postMessage(message('wake'));
      await delay(100);
      equal(events.length, 1, 'busy/stale/malformed messages produce no reply flood');
      worker.postMessage(message('cancel'));
      await delay(50);
      equal(events.length, 1, 'incorrect ACK cannot grant credit');
      worker.postMessage(message('ack', { sequence: 1 }));
      await until(() => events.length === 2, 'cancel terminal after correct credit');
      equal(events[1].response.error.code, 'KG_RUNTIME_CANCELLED', 'valid cancellation');
      worker.postMessage(message('ack', { sequence: 2 }));
      worker.postMessage({ ...run, runGeneration: 12, source: 'fn main() -> i32 { 42 }' });
      worker.postMessage(message('cancel'));
      worker.postMessage(message('ack', { sequence: 2 }));
      await until(() => events.length === 3, 'next generation runnable');
      equal(events[2].runGeneration, 12, 'new run identity');
      worker.postMessage(message('ack', { runGeneration: 12, sequence: 1 }));
      await until(() => events.length === 4, 'next generation complete');
      assertResult(events[3]);
      return { malformedMessages: 11, staleAndBusyDropped: true, nextRun: events[3].response.result };
    }
    if (name === 'client-drops-retired-and-duplicate-replies') {
      const client = make({ workerURL: '/kagari/wasm/fixtures/stale-replies.js' });
      assertResult(await client.run('fn main() -> i32 { 42 }'));
      assertResult(await client.run('fn main() -> i32 { 42 }'));
      client.hardStop();
      assertResult(await client.run('fn main() -> i32 { 42 }'));
      await delay(50);
      const replies = events.filter((event) => event.sequence > 0);
      equal(replies.length, 6, 'exactly one progress/result for each real run');
      equal(replies.map((event) => [event.workerGeneration, event.runGeneration, event.sequence]),
        [[1, 1, 1], [1, 1, 2], [1, 2, 1], [1, 2, 2], [2, 3, 1], [2, 3, 2]], 'only current identities reach consumer');
      return { delivered: replies.length, injectedRetiredOrDuplicate: replies.length * 3, recoveredGeneration: client.workerGeneration };
    }
    if (name === 'waiting-parks-and-cancel-wakes') {
      const client = make({ workerURL: '/kagari/wasm/fixtures/waiting.js' });
      const run = client.run('fn main() { loop {} }');
      await until(() => events.some((event) => event.response?.status === 'waiting'), 'controlled Waiting boundary');
      const parkedCount = events.length;
      await delay(200);
      equal(events.length, parkedCount, 'Waiting does not busy-poll or publish progress');
      client.cancel();
      const cancelled = await run;
      equal(cancelled.response.error.code, 'KG_RUNTIME_CANCELLED', 'cancellation wakes real drive for cleanup');
      client.hardStop();
      assertResult(await client.run('fn main() -> i32 { 42 }'));
      return { fixture: 'Adapter drive reports Waiting; repeat polling throws. No actual external providers installed.', parkedMilliseconds: 200, cancellation: cancelled.response.error.code };
    }
    if (name === 'hard-stop-nonyielding-compile') {
      const client = make({ workerURL: '/kagari/wasm/fixtures/compile-block.js' });
      const blocked = client.run('fn main() -> i32 { 42 }');
      await until(() => events.some((event) => event.type === 'progress'), 'fixture entered synchronous compile boundary');
      let ticks = 0;
      const timer = setInterval(() => { ticks += 1; }, 5);
      try {
        const reset = client.reset();
        await delay(100);
        check(client.busy, 'normal cancellation/reset cannot interrupt synchronous compile');
        check(ticks >= 3, 'compile does not freeze main thread');
        const stopped = client.hardStop();
        equal((await blocked).type, 'stopped', 'run settles on hard termination');
        equal((await reset).type, 'stopped', 'pending reset settles on hard termination');
        assertResult(await client.run('fn main() -> i32 { 42 }'));
        equal(client.workerGeneration, stopped.workerGeneration + 1, 'fresh Worker generation');
        return { fixture: 'Test-only BrowserSession.compile wrapper blocks synchronously before invoking compiler; generation 2 uses real compiler.', mainTaskTicks: ticks, retiredGeneration: stopped.workerGeneration, recoveredGeneration: client.workerGeneration };
      } finally { clearInterval(timer); }
    }
    if (name === 'fatal-drive-fresh-instance' || name === 'malformed-adapter-fresh-instance') {
      const fixture = name === 'fatal-drive-fresh-instance' ? 'fatal-drive' : 'malformed-response';
      const client = make({ workerURL: `/kagari/wasm/fixtures/${fixture}.js` });
      const fatal = await client.run('fn main() -> i32 { 42 }');
      equal(fatal.type, 'fatal', 'unexpected adapter failure invalidates instance');
      check(fatal.message.includes(fixture === 'fatal-drive' ? 'controlled fatal drive' : 'Invalid adapter'), 'specific fatal evidence');
      assertResult(await client.run('fn main() -> i32 { 42 }'));
      equal(client.workerGeneration, fatal.workerGeneration + 1, 'fatal recovery uses fresh instance');
      return { fixture, fatal: fatal.message, recoveredGeneration: client.workerGeneration, unwindClaim: 'None: controlled JS RuntimeError/invalid response, not Rust panic recovery.' };
    }
    throw new Error(`Unknown case: ${name}`);
  } finally {
    for (const client of clients) client.dispose();
    for (const worker of workers) worker.terminate();
  }
}

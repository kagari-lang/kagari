import { LIMITS, VERSION, byteLength, validEvent, validateSlice, validateSource } from './protocol.js';

/** Browser execution client (no DOM/rendering):
 * new PlaygroundClient({ onEvent, slice = 1000, workerURL, productURL }).
 * URLs are trusted application assets, never editor/script input. Defaults put
 * worker.js beside this module and source/kagari_web.js beside playground/.
 * Construction is lazy: no Worker or WASM download until run/reset/hardStop.
 *
 * run(source) synchronously rejects invalid/busy/disposed admission, otherwise
 * returns Promise<terminal event>. There is no queued run. Runtime/compile errors
 * resolve as result events, not rejected promises. Tagged i64/u64 strings and
 * diagnostic UTF-16 positions pass through unchanged. Render all text as text.
 *
 * onEvent(event) receives progress/result/reset/fatal/stopped/disposed. It may
 * return a Promise: the next worker response waits for it, NOT execution/cleanup.
 * Do not await run/reset from inside onEvent (that would hold their response ACK).
 * A throwing/rejecting observer retires the Worker and settles the run as fatal.
 * Progress/output are deltas; output_truncated is sticky for the entire run.
 * The terminal event contains only the remaining output, not a repeated transcript.
 *
 * cancel(): boolean, idempotent cooperative request, run resolves after cleanup.
 * reset(): Promise<event>, coalesced; cancels/drives an active run, then resets.
 *   Admission stays closed until reset completes. Run still receives cancellation.
 * hardStop(): event, immediately terminates/replaces Worker and settles outstanding
 *   operations as stopped. No destructors, rollback or external cleanup promised.
 * dispose(): event, idempotent hard termination with no replacement; final.
 * Fatal faults terminate before notifying; the next run lazily uses a fresh Worker.
 * Read-only busy/disposed/workerGeneration/runGeneration support UI state and
 * deferred-render guards. Each event identifies those generations; local lifecycle events
 * have sequence 0. An in-progress old observer cannot be recalled; UI code that
 * defers rendering itself must compare generations before committing that work.
 */
export class PlaygroundClient {
  #onEvent;
  #slice;
  #workerURL;
  #productURL;
  #worker;
  #generation = 1;
  #runGeneration = 0;
  #active;
  #reset;
  #disposed = false;
  #disposedEvent;

  constructor({ onEvent = () => {}, slice = LIMITS.defaultSlice,
    workerURL = new URL('./worker.js', import.meta.url),
    productURL = new URL('../source/kagari_web.js', import.meta.url) } = {}) {
    if (typeof onEvent !== 'function') throw new TypeError('onEvent must be a function');
    this.#slice = validateSlice(slice);
    this.#onEvent = onEvent;
    this.#workerURL = new URL(workerURL, import.meta.url);
    this.#productURL = new URL(productURL, import.meta.url).href;
    if (this.#productURL.length > LIMITS.urlBytes || byteLength(this.#productURL) > LIMITS.urlBytes) throw new RangeError('productURL exceeds protocol limit');
  }

  get workerGeneration() { return this.#generation; }
  get runGeneration() { return this.#runGeneration; }
  get busy() { return Boolean(this.#active || this.#reset); }
  get disposed() { return this.#disposed; }

  #assertLive() {
    if (this.#disposed) throw new Error('PlaygroundClient is disposed');
  }

  #ensureWorker() {
    if (this.#worker) return;
    const worker = new Worker(this.#workerURL, { type: 'module', name: 'kagari-playground' });
    this.#worker = worker;
    const generation = this.#generation;
    worker.addEventListener('message', ({ data }) => {
      if (worker === this.#worker && generation === this.#generation) void this.#receive(data);
    });
    worker.addEventListener('error', (event) => {
      event.preventDefault();
      if (worker === this.#worker) this.#retire('fatal', 'Worker initialization or execution failed');
    });
    worker.addEventListener('messageerror', () => {
      if (worker === this.#worker) this.#retire('fatal', 'Worker response could not be decoded');
    });
  }

  #post(type, extra = {}) {
    this.#worker.postMessage({ version: VERSION, type, workerGeneration: this.#generation,
      runGeneration: this.#active.id, ...extra });
  }

  #begin(type, extra = {}) {
    const id = ++this.#runGeneration;
    if (!Number.isSafeInteger(id)) throw new Error('Run generation exhausted; create a new client');
    let resolve;
    const promise = new Promise((accept) => { resolve = accept; });
    this.#active = { id, kind: type, resolve, promise, sequence: 0, receiving: false, cancelled: false, bytes: 0, messages: 0, truncated: false };
    try { this.#ensureWorker(); this.#post(type, extra); }
    catch (error) { this.#retire('fatal', String(error.message ?? error).slice(0, 1000)); }
    return promise;
  }

  run(source) {
    this.#assertLive();
    if (this.busy) throw new Error('PlaygroundClient is busy; no run queue is installed');
    validateSource(source);
    return this.#begin('run', { source, slice: this.#slice, productURL: this.#productURL });
  }

  cancel() {
    this.#assertLive();
    if (!this.#active || this.#active.cancelled) return false;
    this.#active.cancelled = true;
    try { this.#post('cancel'); }
    catch (error) { this.#retire('fatal', String(error.message ?? error).slice(0, 1000)); }
    return true;
  }

  reset() {
    this.#assertLive();
    if (this.#reset) return this.#reset.promise;
    const generation = this.#generation;
    let resolve;
    const promise = new Promise((accept) => { resolve = accept; });
    const reset = { promise, resolve };
    this.#reset = reset;
    const previous = this.#active?.promise;
    if (this.#active) this.cancel();
    void (async () => {
      if (previous) await previous;
      if (this.#reset !== reset || generation !== this.#generation) return;
      const event = await this.#begin('reset');
      if (this.#reset !== reset) return;
      this.#reset = undefined;
      resolve(event);
    })();
    return promise;
  }

  async #receive(event) {
    const active = this.#active;
    // Old workers/runs/sequences are ignored, including queued observer continuations.
    if (!active || event?.workerGeneration !== this.#generation || event?.runGeneration !== active.id) return;
    if (Number.isSafeInteger(event.sequence) && event.sequence > 0 && event.sequence <= active.sequence) return;
    if (!validEvent(event)) { this.#retire('fatal', 'Malformed Worker response'); return; }
    if (event.type === 'fatal') { this.#retire('fatal', event.message); return; }
    if ((active.kind === 'reset') !== (event.type === 'reset') || active.receiving || event.sequence !== active.sequence + 1) {
      this.#retire('fatal', 'Worker violated request kind/response credit/sequence');
      return;
    }
    active.bytes += event.response.output.reduce((sum, item) => sum + byteLength(item), 0);
    active.messages += event.response.output.length;
    if (active.bytes > LIMITS.outputBytes || active.messages > LIMITS.outputMessages
        || (active.truncated && !event.response.output_truncated)) {
      this.#retire('fatal', 'Worker violated cumulative output limits');
      return;
    }
    active.truncated ||= event.response.output_truncated;
    active.sequence = event.sequence;
    active.receiving = true;
    const { type, sequence, workerGeneration } = event;
    try {
      await this.#onEvent(event);
      if (this.#active !== active || workerGeneration !== this.#generation) return;
      // ACK before resolving/calling a new run: FIFO clears the worker's old owner.
      this.#post('ack', { sequence });
      active.receiving = false;
      if (type !== 'progress') { this.#active = undefined; active.resolve(event); }
    } catch (error) {
      if (this.#active === active) this.#retire('fatal', `Event delivery failed: ${String(error.message ?? error).slice(0, 900)}`, false);
    }
  }

  #retire(type, message, notify = true) {
    const event = { version: VERSION, type, workerGeneration: this.#generation,
      runGeneration: this.#active?.id ?? this.#runGeneration, sequence: 0, message };
    this.#worker?.terminate();
    this.#worker = undefined;
    this.#generation += 1;
    const active = this.#active;
    const reset = this.#reset;
    this.#active = undefined;
    this.#reset = undefined;
    active?.resolve(event);
    reset?.resolve(event);
    // Lifecycle notifications do not hold an ACK. A broken observer cannot prevent
    // termination or create an unhandled rejection by also rejecting this notice.
    if (notify) {
      try { Promise.resolve(this.#onEvent(event)).catch(() => {}); } catch { /* Already retired. */ }
    }
    return event;
  }

  hardStop() {
    this.#assertLive();
    const event = this.#retire('stopped', 'Worker terminated; completed effects are not rolled back');
    try { this.#ensureWorker(); }
    catch (error) { this.#retire('fatal', String(error.message ?? error).slice(0, 1000)); }
    return event;
  }

  dispose() {
    if (this.#disposed) return this.#disposedEvent;
    this.#disposed = true;
    this.#disposedEvent = this.#retire('disposed', 'Client disposed');
    return this.#disposedEvent;
  }
}

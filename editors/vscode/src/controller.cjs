'use strict';

// One owner serializes replacement, connection recovery and shutdown. LanguageClient
// disposes its providers before recovery is admitted; it never restarts itself.
const delays = [250, 1000, 4000];

class ClientController {
  constructor({ create, report, changed, clock = { now: () => Date.now(), setTimeout, clearTimeout } }) {
    this.create = create;
    this.report = report;
    this.changed = changed;
    this.clock = clock;
    this.generation = 0;
    this.failures = [];
    this.queue = Promise.resolve();
    this.current = undefined;
    this.timer = undefined;
    this.disposed = false;
    this.status = Object.freeze({ state: 'disabled', restartCount: 0, pendingRestart: false });
  }

  publish(state, error) {
    this.status = Object.freeze({ state, error, generation: this.generation,
      restartCount: this.failures.length, pendingRestart: this.timer !== undefined,
      ...this.current?.adapter?.status() });
    this.changed(this.status);
  }

  enqueue(operation) {
    this.queue = this.queue.then(operation).catch((error) => {
      this.report(error.message);
      this.publish('error', error.message);
    });
    return this.queue;
  }

  cancelTimer() {
    if (this.timer !== undefined) this.clock.clearTimeout(this.timer);
    this.timer = undefined;
  }

  configure(configuration) {
    if (this.disposed) return this.queue;
    this.configuration = configuration;
    const generation = ++this.generation;
    this.failures = [];
    this.cancelTimer();
    return this.enqueue(async () => {
      await this.retire();
      if (generation !== this.generation) return;
      if (!configuration.enabled) return this.publish('disabled');
      await this.launch(generation);
    });
  }

  restart() {
    return this.configure(this.configuration);
  }

  async retire() {
    const record = this.current;
    if (!record) return;
    record.intentional = true;
    await record.adapter.stop();
    if (this.current === record) this.current = undefined;
  }

  async launch(generation) {
    if (this.disposed || generation !== this.generation) return;
    const record = { generation, intentional: false, lost: false, adapter: undefined };
    this.current = record;
    try {
      record.adapter = await this.create(this.configuration, () => this.closed(record));
      if (generation !== this.generation || this.disposed) return this.retire();
      this.publish('starting');
      await record.adapter.start();
      if (generation !== this.generation || this.disposed) return this.retire();
      if (!record.lost) this.publish('running');
    } catch (error) {
      if (!record.adapter) this.current = undefined;
      if (generation !== this.generation || this.disposed) return;
      this.report(error.message);
      if (error.configuration || !record.adapter) {
        await this.retire();
        this.publish('error', error.message);
      } else {
        this.closed(record);
      }
    }
  }

  closed(record) {
    if (record !== this.current || record.intentional || record.lost
      || this.disposed || record.generation !== this.generation) return;
    record.lost = true;
    this.publish('recovering');
    // The library close callback must return immediately. Its own cleanup and
    // Stopped/StartFailed event happen before this queued recovery can continue.
    void this.enqueue(async () => {
      await record.adapter.waitStopped();
      if (record !== this.current || record.generation !== this.generation || this.disposed) return;
      await this.retire();
      const now = this.clock.now();
      this.failures = this.failures.filter((time) => now - time < 60000);
      if (this.failures.length >= delays.length) {
        const message = 'Kagari stopped after three automatic restarts in one minute. Use Kagari: Restart Language Server.';
        this.report(message);
        this.publish('error', message);
        return;
      }
      const delay = delays[this.failures.length];
      this.failures.push(now);
      const generation = record.generation;
      this.timer = this.clock.setTimeout(() => {
        this.timer = undefined;
        void this.enqueue(() => this.launch(generation));
      }, delay);
      this.report(`Kagari connection closed; restart ${this.failures.length}/3 in ${delay} ms.`);
      this.publish('recovering');
    });
  }

  dispose() {
    this.disposed = true;
    ++this.generation;
    this.cancelTimer();
    return this.enqueue(async () => {
      await this.retire();
      this.publish('stopped');
    });
  }
}

module.exports = { ClientController };

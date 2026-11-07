import assert from 'node:assert/strict';
import test from 'node:test';
import { createRequire } from 'node:module';

const { ClientController } = createRequire(import.meta.url)('../../vscode/src/controller.cjs');

function fixture() {
  let now = 0;
  let serial = 0;
  const timers = new Map();
  const clients = [];
  const states = [];
  const configuration = { enabled: true, path: '/explicit/analyzer' };
  const clock = {
    now: () => now,
    setTimeout: (callback, delay) => { const id = ++serial; timers.set(id, { callback, at: now + delay }); return id; },
    clearTimeout: (id) => timers.delete(id),
  };
  const controller = new ClientController({ clock, report: () => {}, changed: (state) => states.push(state),
    create: async (config, closed) => {
      assert.equal(config.path, configuration.path);
      let stopped;
      const wait = new Promise((resolve) => { stopped = resolve; });
      const client = {
        stopped: false,
        start: async () => {},
        stop: async () => { client.stopped = true; stopped(); },
        waitStopped: () => wait,
        status: () => ({ pid: clients.indexOf(client) + 1 }),
        crash: () => { closed(); return stopped; },
      };
      clients.push(client);
      return client;
    },
  });
  async function advance(milliseconds) {
    now += milliseconds;
    for (const [id, timer] of timers) {
      if (timer.at <= now) { timers.delete(id); timer.callback(); }
    }
    await controller.queue;
  }
  return { controller, configuration, clients, states, timers, advance };
}

test('recovery waits for library cleanup and admits only three rolling-minute restarts', async () => {
  const f = fixture();
  await f.controller.configure(f.configuration);
  for (const delay of [250, 1000, 4000]) {
    const prior = f.clients.at(-1);
    const cleaned = prior.crash();
    await Promise.resolve();
    assert.equal(f.timers.size, 0, 'backoff cannot hide live registrations');
    cleaned();
    await f.controller.queue;
    assert.equal(prior.stopped, true);
    const count = f.clients.length;
    await f.advance(delay - 1);
    assert.equal(f.clients.length, count);
    await f.advance(1);
    assert.equal(f.clients.length, count + 1);
  }
  f.clients.at(-1).crash()();
  await f.controller.queue;
  assert.equal(f.controller.status.state, 'error');
  assert.equal(f.timers.size, 0);
  assert.equal(f.clients.length, 4);
  await f.controller.restart();
  assert.equal(f.controller.status.state, 'running');
  assert.equal(f.controller.status.restartCount, 0);
  await f.controller.dispose();
  assert.ok(f.clients.every((client) => client.stopped));
});

test('manual replacement and disable cancel old timers and ignore old close callbacks', async () => {
  const f = fixture();
  await f.controller.configure(f.configuration);
  const old = f.clients[0];
  old.crash()();
  await f.controller.queue;
  assert.equal(f.timers.size, 1);
  await f.controller.restart();
  old.crash()();
  await f.advance(60000);
  assert.equal(f.clients.length, 2);
  f.clients.at(-1).crash()();
  await f.controller.queue;
  await f.controller.configure({ ...f.configuration, enabled: false });
  await f.advance(60000);
  assert.equal(f.clients.length, 2);
  assert.equal(f.controller.status.state, 'disabled');
  assert.equal(f.timers.size, 0);
  await f.controller.dispose();
});

test('invalid explicit path stops without automatic recovery', async () => {
  const error = Object.assign(new Error('missing analyzer'), { configuration: true });
  const controller = new ClientController({ create: async () => { throw error; }, report: () => {}, changed: () => {} });
  await controller.configure({ enabled: true, path: '/missing' });
  assert.equal(controller.status.state, 'error');
  assert.equal(controller.status.pendingRestart, false);
  assert.equal(controller.status.restartCount, 0);
  await controller.dispose();
});

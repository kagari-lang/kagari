/** Browser-only fault wrappers. Served from the harness snapshot, never shipped
 * in playground/. They intercept the real adapter at non-preemptible boundaries;
 * there are no test switches, arbitrary callbacks or fixture URLs in production.
 */
export function workerFixture(kind) {
  if (!['compile-block', 'fatal-drive', 'waiting', 'malformed-response', 'stale-replies'].includes(kind)) throw new Error('Unknown fixture');
  return `
import init, { BrowserSession } from '../source/kagari_web.js';
import { VERSION, emptyResponse } from '../playground/protocol.js';
let booting = true;
const pending = [];
let request;
let cancelled = false;
let parked = false;
self.addEventListener('message', (event) => {
  const { data } = event;
  if (booting) {
    if (pending.length >= 4) throw new Error('Fixture startup admission exceeded');
    pending.push(data);
    event.stopImmediatePropagation();
    return;
  }
  if (data.type === 'run') request = data;
  if (data.type === 'cancel' && request && data.workerGeneration === request.workerGeneration && data.runGeneration === request.runGeneration) cancelled = true;
});
if (${JSON.stringify(kind)} === 'stale-replies') {
  const post = self.postMessage.bind(self);
  self.postMessage = (message) => {
    // The replacement worker can deliver otherwise well-formed retired identities.
    post({ ...message, workerGeneration: message.workerGeneration - 1 });
    post({ ...message, runGeneration: message.runGeneration - 1 });
    post(message);
    post(message); // A duplicate must never grant another response credit.
  };
}
await init();
const compile = BrowserSession.prototype.compile;
const drive = BrowserSession.prototype.drive;
BrowserSession.prototype.compile = function(source) {
  if (${JSON.stringify(kind)} === 'compile-block' && request.workerGeneration === 1) {
    self.postMessage({ version: VERSION, type: 'progress', workerGeneration: request.workerGeneration,
      runGeneration: request.runGeneration, sequence: 1, response: emptyResponse('runnable') });
    // Controlled synchronous compiler-boundary stand-in. Only terminate can exit.
    while (true) {}
  }
  return compile.call(this, source);
};
BrowserSession.prototype.drive = function(slice) {
  if (request.workerGeneration === 1) {
    if (${JSON.stringify(kind)} === 'fatal-drive') throw new WebAssembly.RuntimeError('controlled fatal drive fixture');
    if (${JSON.stringify(kind)} === 'malformed-response') return '{"version":1}';
    if (${JSON.stringify(kind)} === 'waiting' && !cancelled) {
      if (parked) throw new Error('Waiting was busy-polled without an owned wake');
      parked = true;
      return JSON.stringify(emptyResponse('waiting'));
    }
  }
  return drive.call(this, slice);
};
await import('../playground/worker.js');
booting = false;
for (const data of pending) self.dispatchEvent(new MessageEvent('message', { data }));
pending.length = 0;
`;
}

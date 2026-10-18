/** Worker protocol v1. Limits have one JS owner; adapter v1 supplies per-drain bounds.
 * These are traffic/admission limits, not compiler or WASM heap quotas.
 */
export const LIMITS = Object.freeze({
  sourceBytes: 256 * 1024,
  outputBytes: 64 * 1024,
  outputMessages: 256,
  messageBytes: 4 * 1024,
  resultBytes: 16 * 1024,
  // Covers adapter maxima, including 32 diagnostics with 8 labels/notes each,
  // bounded locations, and worst-case six-character JSON escapes per text byte.
  responseCharacters: 24 * 1024 * 1024,
  urlBytes: 4096,
  maxSlice: 100_000,
  defaultSlice: 1_000,
  flushMilliseconds: 32,
});
export const VERSION = 1;
const encoder = new TextEncoder();
export const byteLength = (text) => encoder.encode(text).length;
export const isGeneration = (value) => Number.isSafeInteger(value) && value > 0;
const record = (value) => value !== null && typeof value === 'object' && !Array.isArray(value);
const text = (value, bytes) => typeof value === 'string' && value.length <= bytes && byteLength(value) <= bytes;

export function validateSlice(slice) {
  if (!Number.isInteger(slice) || slice <= 0 || slice > LIMITS.maxSlice) {
    throw new RangeError(`slice must be a positive finite integer <= ${LIMITS.maxSlice}`);
  }
  return slice;
}

/** Check length before encoding, so an already oversized caller string is not copied. */
export function validateSource(source) {
  if (typeof source !== 'string') throw new TypeError('source must be a string');
  if (source.length > LIMITS.sourceBytes || byteLength(source) > LIMITS.sourceBytes) {
    throw new RangeError(`source exceeds ${LIMITS.sourceBytes} UTF-8 bytes`);
  }
  return source;
}

export function validCommand(value) {
  if (!record(value) || value.version !== VERSION || !isGeneration(value.workerGeneration)
      || !isGeneration(value.runGeneration)) return false;
  if (value.type === 'ack') return isGeneration(value.sequence);
  if (['cancel', 'reset'].includes(value.type)) return true;
  if (value.type !== 'run' || !text(value.productURL, LIMITS.urlBytes)) return false;
  try { validateSlice(value.slice); validateSource(value.source); return true; }
  catch { return false; }
}

function validLocation(value) {
  if (value === null) return true;
  const position = (item) => record(item) && ['offset', 'line', 'character'].every((key) => Number.isSafeInteger(item[key]) && item[key] >= 0);
  return record(value) && text(value.origin, LIMITS.messageBytes) && text(value.revision, 128)
    && ['byte_start', 'byte_end'].every((key) => Number.isSafeInteger(value[key]) && value[key] >= 0)
    && position(value.start) && position(value.end);
}

function validDiagnostic(value) {
  return record(value) && text(value.code, 128) && ['error', 'warning'].includes(value.severity)
    && text(value.message, LIMITS.messageBytes) && typeof value.truncated === 'boolean'
    && validLocation(value.location) && Array.isArray(value.labels) && value.labels.length <= 8
    && value.labels.every((item) => record(item) && validLocation(item.location) && text(item.message, LIMITS.messageBytes))
    && Array.isArray(value.notes) && value.notes.length <= 8 && value.notes.every((item) => text(item, LIMITS.messageBytes));
}

function validResult(value) {
  if (value === null) return true;
  if (!record(value)) return false;
  switch (value.tag) {
    case 'unit': return true;
    case 'bool': return typeof value.value === 'boolean';
    case 'i32': case 'i64': case 'u64': return text(value.value, 32) && /^-?\d+$/u.test(value.value);
    case 'f32': case 'f64': return text(value.value, 128);
    case 'string': return text(value.value, LIMITS.resultBytes) && typeof value.truncated === 'boolean';
    case 'opaque': return ['tuple', 'range', 'array', 'map', 'set', 'enum', 'struct', 'gc_handle', 'interface', 'closure', 'cell', 'host_root', 'host_path_view', 'ephemeral'].includes(value.kind);
    default: return false;
  }
}

export function validResponse(value) {
  return record(value) && value.version === VERSION
    && ['idle', 'ready', 'runnable', 'waiting', 'complete', 'error', 'output'].includes(value.status)
    && Array.isArray(value.output) && value.output.length <= LIMITS.outputMessages
    && value.output.every((item) => text(item, LIMITS.messageBytes))
    && value.output.reduce((sum, item) => sum + byteLength(item), 0) <= LIMITS.outputBytes
    && typeof value.output_truncated === 'boolean' && validResult(value.result)
    && (value.error === null || (record(value.error) && text(value.error.code, 128)
      && value.error.severity === 'error' && text(value.error.message, LIMITS.messageBytes) && typeof value.error.truncated === 'boolean'))
    && Array.isArray(value.diagnostics) && value.diagnostics.length <= 32
    && value.diagnostics.every(validDiagnostic) && typeof value.diagnostics_truncated === 'boolean';
}

export function parseResponse(raw) {
  if (typeof raw !== 'string' || raw.length > LIMITS.responseCharacters) throw new Error('Invalid adapter response size');
  const value = JSON.parse(raw);
  if (!validResponse(value)) throw new Error('Invalid adapter v1 response');
  return value;
}

export function validEvent(value) {
  if (!record(value) || value.version !== VERSION || !isGeneration(value.workerGeneration)
      || !isGeneration(value.runGeneration) || !isGeneration(value.sequence)) return false;
  if (value.type === 'fatal') return text(value.message, LIMITS.messageBytes);
  if (!validResponse(value.response)) return false;
  return value.type === 'progress' ? ['runnable', 'waiting'].includes(value.response.status)
    : value.type === 'result' ? ['complete', 'error'].includes(value.response.status)
      : value.type === 'reset' && ['idle', 'error'].includes(value.response.status);
}

export function emptyResponse(status) {
  return { version: VERSION, status, output: [], output_truncated: false, result: null, error: null, diagnostics: [], diagnostics_truncated: false };
}

/** Retain only unsent deltas. Quotas count all drains, including already sent output.
 * Once the JS quota cannot fit a message, visibly drop the rest. Adapter-level
 * truncation (including individual long messages) is sticky but does not exhaust
 * the JS quota: later drains can still contribute output.
 */
export class OutputBudget {
  bytes = 0;
  messages = 0;
  truncated = false;
  exhausted = false;
  pending = [];

  append(response) {
    for (const item of response.output) {
      const bytes = byteLength(item);
      if (this.messages >= LIMITS.outputMessages || this.bytes + bytes > LIMITS.outputBytes || this.exhausted) {
        this.truncated = true;
        this.exhausted = true;
        continue;
      }
      this.messages += 1;
      this.bytes += bytes;
      this.pending.push(item);
    }
    this.truncated ||= response.output_truncated;
  }

  drain(response) {
    const result = { ...response, output: this.pending, output_truncated: this.truncated };
    this.pending = [];
    return result;
  }
}

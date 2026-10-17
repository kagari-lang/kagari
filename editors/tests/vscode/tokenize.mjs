import { readFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import textmate from 'vscode-textmate';
import oniguruma from 'vscode-oniguruma';

const require = createRequire(import.meta.url);
const grammarUrl = new URL('../../vscode/syntaxes/kagari.tmLanguage.json', import.meta.url);
const onigLib = (async () => {
  const wasm = await readFile(require.resolve('vscode-oniguruma/release/onig.wasm'));
  await oniguruma.loadWASM(wasm.buffer.slice(wasm.byteOffset, wasm.byteOffset + wasm.byteLength));
  return {
    createOnigScanner: (patterns) => new oniguruma.OnigScanner(patterns),
    createOnigString: (text) => new oniguruma.OnigString(text),
  };
})();

/** Load the shipped grammar, optionally with a real TextMate theme. */
export async function loadGrammar(theme) {
  const registry = new textmate.Registry({
    onigLib,
    theme,
    loadGrammar: async (scope) => scope === 'source.kagari'
      ? textmate.parseRawGrammar(await readFile(grammarUrl, 'utf8'), fileURLToPath(grammarUrl))
      : null,
  });
  const grammar = await registry.loadGrammar('source.kagari');
  if (!grammar) throw new Error('The shipped source.kagari grammar did not load');
  return { grammar, registry };
}

let defaultGrammar;

/** Decode VS Code StandardTokenType, not a heuristic over scope names. */
export function tokenType(metadata) {
  const type = (metadata >>> 8) & 3;
  if (type === 0) return 'code';
  if (type === 1) return 'comment';
  if (type === 2) return 'string';
  throw new Error(`Unexpected regex token category: ${metadata}`);
}

/**
 * Carry actual rule stacks through lines, retaining scoped and binary tokens.
 * Offsets are UTF-16. LF separators are excluded; CR in CRLF is retained.
 * Call with a valid preceding stack/offset to exercise incremental retokenization.
 */
export function scanLines(lines, grammar, initialStack = textmate.INITIAL, firstOffset = 0) {
  const spans = [];
  const rows = [];
  let stack = initialStack;
  let offset = firstOffset;
  for (const line of lines) {
    const scoped = grammar.tokenizeLine(line, stack);
    const encoded = grammar.tokenizeLine2(line, stack);
    if (scoped.stoppedEarly || encoded.stoppedEarly) {
      throw new Error(`TextMate tokenization stopped early at offset ${offset}`);
    }
    rows.push({ text: line, offset, stackBefore: stack, stackAfter: scoped.ruleStack,
      tokens: scoped.tokens, encoded: encoded.tokens });
    let binaryIndex = 0;
    for (const token of scoped.tokens) {
      let start = token.startIndex;
      const end = Math.min(token.endIndex, line.length);
      while (start < end) {
        while (binaryIndex + 2 < encoded.tokens.length && encoded.tokens[binaryIndex + 2] <= start) {
          binaryIndex += 2;
        }
        const next = Math.min(end, encoded.tokens[binaryIndex + 2] ?? line.length);
        spans.push({ start: offset + start, end: offset + next,
          scopes: token.scopes, type: tokenType(encoded.tokens[binaryIndex + 1]) });
        start = next;
      }
    }
    stack = scoped.ruleStack;
    offset += line.length + 1;
  }
  return { spans, rows };
}

export async function tokenizeDocument(text, grammar) {
  if (!grammar) {
    defaultGrammar ??= loadGrammar();
    ({ grammar } = await defaultGrammar);
  }
  return scanLines(text.split('\n'), grammar);
}

/** Tokenize source with the actual shipped grammar; spans use UTF-16 offsets. */
export async function tokenize(text) {
  return (await tokenizeDocument(text)).spans;
}

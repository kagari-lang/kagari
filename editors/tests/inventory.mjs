import { readdir, readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { join, relative } from 'node:path';
import Parser from 'tree-sitter';
import Kagari from 'tree-sitter-kagari';

// The complete example inventory belongs to CI; local tests select contract cases.
const repository = fileURLToPath(new URL('../../', import.meta.url));
const examples = join(repository, 'examples', 'syntax');
const invalidTerminals = new Set(['unterminated_string_end', 'unterminated_block_comment']);

async function collect(directory) {
  const files = [];
  const entries = await readdir(directory, { withFileTypes: true });
  entries.sort((a, b) => a.name.localeCompare(b.name, 'en'));
  for (const entry of entries) {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) files.push(...await collect(path));
    else if (entry.isFile() && entry.name.endsWith('.kgr')) files.push(path);
  }
  return files;
}

const files = await collect(examples);
if (files.length === 0) throw new Error('No examples/syntax/**/*.kgr files found');
const parser = new Parser();
parser.setLanguage(Kagari);
let failed = 0;
for (const file of files) {
  const source = await readFile(file, 'utf8');
  const tree = parser.parse(source);
  if (!tree) throw new Error(`Native parser did not return a tree for ${file}`);
  const pending = [tree.rootNode];
  const problems = [];
  while (pending.length > 0) {
    const node = pending.pop();
    // Recovery terminals are invalid even when Tree-sitter reports no ERROR.
    if (node.type === 'ERROR' || node.isMissing || invalidTerminals.has(node.type)) {
      const { row, column } = node.startPosition;
      problems.push(`${row + 1}:${column + 1}: ${node.isMissing ? 'MISSING ' : ''}${node.type} ${JSON.stringify(node.text.slice(0, 80))}`);
    }
    for (let index = node.childCount - 1; index >= 0; index--) {
      pending.push(node.child(index));
    }
  }
  if (tree.rootNode.hasError && problems.length === 0) {
    problems.push('Native parser reported an unclassified syntax error');
  }
  if (problems.length > 0) {
    failed++;
    console.error(`${relative(repository, file)}:\n  ${problems.join('\n  ')}`);
  }
}
console.log(`Native grammar inventory: ${files.length - failed}/${files.length} syntax examples passed.`);
if (failed > 0) process.exitCode = 1;

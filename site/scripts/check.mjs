import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { readFile, readdir } from 'node:fs/promises';
import { join, relative } from 'node:path';
import { base, page, site, source } from '../src/links.mjs';
import { pages } from '../src/navigation.mjs';
import { readExample, repositoryRoot } from '../src/source-example.mjs';
import { distRoot, inputDigest, manifestName, outputDigest, siteRoot, walk } from './files.mjs';
import { codeText, elements, text, validateHtml } from './html.mjs';
import { isFenceLanguage } from './fences.mjs';

const args = process.argv.slice(2);
assert(args.length === 0 || (args.length === 2 && args[0] === '--expected-pages' && /^\d+$/u.test(args[1])), 'Usage: npm run check -- [--expected-pages N]');
if (args.length) assert.equal(pages.length, Number(args[1]), 'Unexpected route count');
assert.equal(new Set(pages.map(({ path }) => path)).size, pages.length, 'Duplicate route inventory');

let manifest;
try { manifest = JSON.parse(await readFile(join(distRoot, manifestName), 'utf8')); }
catch { throw new Error('Missing successful build record. Run npm run build before npm run check.'); }
assert.equal(manifest.inputs, await inputDigest(), 'Stale build: inputs changed; run npm run build.');
assert.equal(manifest.output, await outputDigest(), 'Build output changed; run npm run build.');

const files = new Map();
for (const path of await walk(distRoot)) {
  const name = relative(distRoot, path).replaceAll('\\', '/');
  files.set(name, /\.(html|css|xml|js)$/u.test(name) ? await readFile(path, 'utf8') : '');
}
const errors = validateHtml(files, { origin: site, base, routes: pages });
let includes = 0;
const languages = new Set();
for (const [name, html] of files) {
  if (!name.endsWith('.html')) continue;
  const nodes = elements(html);
  for (const { node, tag, attrs } of nodes) {
    if (attrs['data-language']) {
      const language = attrs['data-language'];
      languages.add(language);
      if (!isFenceLanguage(language)) errors.push(`${name}: unknown rendered fence language ${language}`);
      if (!['text', 'plaintext'].includes(language)) {
        // Highlighted blocks must contain actual token styles, not just a language label.
        function highlighted(current) {
          return (current.attrs?.some(({ name: key, value }) => key === 'style' && /(?:color|--[\w-]+):/u.test(value)) ?? false) || current.childNodes?.some(highlighted);
        }
        if (!highlighted(node)) errors.push(`${name}: unhighlighted ${language} block`);
      }
    }
    if (tag !== 'figure' || !attrs['data-source-example']) continue;
    includes += 1;
    const example = await readExample(attrs['data-source-example']);
    const descendants = [];
    function visit(current) { descendants.push(current); current.childNodes?.forEach(visit); }
    visit(node);
    const code = descendants.find((current) => current.tagName === 'code');
    if (!code || codeText(code).trimEnd() !== example.code) errors.push(`${name}: source include differs from ${example.path}`);
    if (!descendants.some((current) => current.tagName === 'a' && current.attrs.some(({ name: key, value }) => key === 'href' && value === source(example.path)))) errors.push(`${name}: source include lacks canonical source link`);
  }
  // Check repository links locally, without making correctness depend on the network.
  for (const { attrs } of nodes) {
    if (!attrs.href?.startsWith(source('README.md').replace('README.md', ''))) continue;
    const path = new URL(attrs.href).pathname.split('/blob/main/')[1];
    try { await readFile(join(repositoryRoot, path)); }
    catch (error) {
      if (error.code !== 'EISDIR') {
        try { await readdir(join(repositoryRoot, path)); }
        catch { errors.push(`${name}: missing repository source ${path}`); }
      }
    }
  }
}
for (const language of ['kagari', 'rust']) if (!languages.has(language)) errors.push(`No rendered ${language} block`);
assert(includes > 0, 'No canonical source examples rendered');
for (const required of ['pagefind/pagefind.js', 'sitemap-index.xml', 'sitemap-0.xml']) if (!files.has(required)) errors.push(`Missing search/sitemap asset ${required}`);
if (![...files.keys()].some((name) => name.startsWith('pagefind/') && name.endsWith('.pf_index'))) errors.push('Missing Pagefind search index');
for (const [name, xml] of files) {
  if (!name.endsWith('.xml')) continue;
  for (const { tag, node } of elements(xml)) {
    if (tag !== 'loc') continue;
    const url = new URL(text(node));
    if (url.origin !== site || !url.pathname.startsWith(`${base}/`)) errors.push(`${name}: sitemap URL outside project base: ${url}`);
    else {
      const path = url.pathname.slice(base.length + 1);
      if (!files.has(path) && !files.has(`${path}index.html`)) errors.push(`${name}: missing sitemap target ${url}`);
    }
  }
}
// Ensure every source document is represented; navigation never advertises unwritten pages.
const content = (await walk(join(siteRoot, 'src/content/docs'))).filter((path) => /\.mdx?$/u.test(path));
assert.equal(content.length, pages.length, 'Content/inventory route counts differ');
for (const route of pages) {
  const slug = route.path || 'index';
  assert(content.some((path) => path === join(siteRoot, `src/content/docs/${slug}.mdx`) || path === join(siteRoot, `src/content/docs/${slug}.md`)), `Missing source for ${page(route.path)}`);
}
assert.deepEqual(errors, [], errors.join('\n'));
const tests = (await readdir(join(siteRoot, 'scripts'))).filter((name) => name.endsWith('.test.mjs')).map((name) => join(siteRoot, 'scripts', name));
const result = spawnSync(process.execPath, ['--test', ...tests], { cwd: siteRoot, stdio: 'inherit', timeout: 120_000 });
assert(!result.error && result.status === 0, `Script tests failed: ${result.error?.message ?? result.status}`);
assert.equal(manifest.inputs, await inputDigest(), 'Build inputs changed during validation.');
assert.equal(manifest.output, await outputDigest(), 'Build output changed during validation.');
console.log(`Validated ${pages.length} guide pages, 404, ${includes} source include(s), links/anchors/assets/base, highlighting and fresh output.`);

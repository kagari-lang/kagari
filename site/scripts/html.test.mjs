import assert from 'node:assert/strict';
import test from 'node:test';
import { codeText, elements, validateHtml } from './html.mjs';

const options = {
  origin: 'https://kagari-lang.github.io',
  base: '/kagari',
  routes: [{ path: '', title: 'Kagari', description: 'The guide.' }],
};

test('rendered code comparison preserves indentation and blank source lines', () => {
  const [{ node }] = elements('<pre><code><div class="ec-line"><div class="code">  first</div></div><div class="ec-line"><div class="code">\n</div></div><div class="ec-line"><div class="code">last</div></div></code></pre>').filter(({ tag }) => tag === 'code');
  assert.equal(codeText(node), '  first\n\nlast');
});

function fixture(body = '') {
  return new Map([
    ['index.html', `<!doctype html><html><head><title>Kagari</title><meta name="description" content="The guide."><link rel="canonical" href="https://kagari-lang.github.io/kagari/"><link rel="stylesheet" href="/kagari/theme&#x2e;css"></head><body><h1>Kagari</h1><h2 id="hello-world">Hello</h2>${body}</body></html>`],
    ['404.html', '<!doctype html><title>Not found</title><a href="/kagari/">Home</a>'],
    ['theme.css', 'body { color: black; }'],
  ]);
}

test('DOM checker resolves entity-decoded URLs and anchors, ignoring comment/script lookalikes', () => {
  const files = fixture('<a href="/kagari/#hello&#x2d;world">Next</a><!-- <a href="/broken"> --><script>const text = \'<a href="/broken">\';</script>');
  assert.deepEqual(validateHtml(files, options), []);
});

test('DOM checker rejects nonexistent local anchors', () => {
  assert(validateHtml(fixture('<a href="/kagari/#absent">Broken</a>'), options).some((error) => error.includes('missing anchor')));
});

test('DOM checker rejects missing assets, including srcset candidates', () => {
  for (const body of ['<img src="/kagari/missing.svg">', '<img srcset="/kagari/theme.css 1x, /kagari/missing.png 2x">']) {
    assert(validateHtml(fixture(body), options).some((error) => error.includes('missing') && error.includes('asset')));
  }
});

test('DOM checker rejects root links and same-origin absolute URLs outside the project base', () => {
  for (const href of ['/start/quickstart/', 'https://kagari-lang.github.io/start/quickstart/', '/kagari/../outside/']) {
    assert(validateHtml(fixture(`<a href="${href}">Broken</a>`), options).some((error) => error.includes('escapes project base')));
  }
});

test('DOM checker checks CSS assets relative to their stylesheet', () => {
  const files = fixture();
  files.set('theme.css', 'body { background: url(./missing.svg); }');
  assert(validateHtml(files, options).some((error) => error.includes('missing CSS asset')));
});

test('only the generated error page may use Starlight\'s synthetic 404 canonical', () => {
  const files = fixture();
  files.set('404.html', '<link rel="canonical" href="https://kagari-lang.github.io/kagari/404/"><a href="/kagari/">Home</a>');
  assert.deepEqual(validateHtml(files, options), []);
  files.set('index.html', files.get('index.html').replace('</body>', '<a href="/kagari/404/">Missing</a></body>'));
  assert(validateHtml(files, options).some((error) => error.includes('missing link target /kagari/404/')));
});

test('DOM checker resolves nested routes and their anchors rather than trusting URL shape', () => {
  const files = fixture('<a href="/kagari/start/quickstart/#run">Run</a>');
  files.set('start/quickstart/index.html', '<h2 id="run">Run</h2><a href="../../#hello-world">Home</a>');
  // This page is deliberately not in the inventory; link resolution itself must still work.
  const errors = validateHtml(files, options);
  assert.equal(errors.length, 1);
  assert.match(errors[0], /absent from navigation inventory/u);
});

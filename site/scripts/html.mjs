import { parse } from 'parse5';

export function text(node) {
  return node.nodeName === '#text' ? node.value : (node.childNodes ?? []).map(text).join('');
}

/** Recover source line boundaries from Expressive Code's visual line containers. */
export function codeText(code) {
  const lines = code.childNodes.filter((node) => node.attrs?.some(({ name, value }) => name === 'class' && value.split(' ').includes('ec-line')));
  if (!lines.length) return text(code);
  return lines.map((line) => {
    const value = text(line);
    // Expressive Code inserts a newline sentinel to give an otherwise empty line height.
    return value === '\n' ? '' : value;
  }).join('\n');
}

export function elements(html) {
  const result = [];
  function visit(node) {
    if (node.tagName) result.push({ node, tag: node.tagName, attrs: Object.fromEntries(node.attrs.map(({ name, value }) => [name, value])) });
    node.childNodes?.forEach(visit);
    if (node.content) visit(node.content);
  }
  visit(parse(html));
  return result;
}

/** Validate real parsed HTML, including entity-decoded attributes and forward anchors. */
export function validateHtml(files, { origin, base, routes }) {
  const errors = [];
  const documents = new Map([...files].filter(([name]) => name.endsWith('.html')).map(([name, html]) => [name, elements(html)]));
  const ids = new Map([...documents].map(([name, nodes]) => [name, new Set(nodes.map(({ attrs }) => attrs.id).filter(Boolean))]));
  const resolveFile = (path) => [path || 'index.html', `${path.replace(/\/?$/u, '/')}index.html`, `${path}.html`].find((name) => files.has(name));

  function validateUrl(value, from, kind, anchors = false) {
    if (!value || /^(?:data:|mailto:|tel:)/iu.test(value)) return;
    let url;
    try { url = new URL(value, `${origin}${base}/${from.replace(/index\.html$/u, '')}`); }
    catch { errors.push(`${from}: invalid ${kind} URL ${value}`); return; }
    if (url.protocol !== 'https:' && url.protocol !== 'http:') {
      errors.push(`${from}: unsupported ${kind} URL ${value}`);
      return;
    }
    if (url.origin !== origin) return;
    if (!url.pathname.startsWith(`${base}/`)) {
      errors.push(`${from}: ${kind} escapes project base: ${value}`);
      return;
    }
    let path;
    try { path = decodeURIComponent(url.pathname.slice(base.length + 1)); }
    catch { errors.push(`${from}: invalid URL encoding ${value}`); return; }
    // Starlight synthesizes /404/ as the error page's canonical, while Pages serves 404.html.
    // This is not a navigable guide route; never exempt ordinary links to /404/.
    if (from === '404.html' && kind === 'canonical' && path === '404/') return;
    const target = resolveFile(path);
    if (!target) {
      errors.push(`${from}: missing ${kind} target ${value}`);
      return;
    }
    if (anchors && url.hash && ids.has(target)) {
      let fragment;
      try { fragment = decodeURIComponent(url.hash.slice(1)); }
      catch { errors.push(`${from}: invalid anchor encoding ${value}`); return; }
      // Text fragments are browser directives, not element IDs.
      fragment = fragment.split(':~:text=')[0];
      if (fragment && !ids.get(target).has(fragment)) errors.push(`${from}: missing anchor ${value}`);
    }
  }

  for (const [name, nodes] of documents) {
    const seen = new Set();
    for (const { tag, attrs } of nodes) {
      if (attrs.id) {
        if (seen.has(attrs.id)) errors.push(`${name}: duplicate ID ${attrs.id}`);
        seen.add(attrs.id);
      }
      if (tag === 'base') errors.push(`${name}: HTML base elements are not supported`);
      if (attrs.href !== undefined) validateUrl(attrs.href, name, tag === 'link' && attrs.rel === 'canonical' ? 'canonical' : 'link', tag === 'a');
      if (attrs.src !== undefined) validateUrl(attrs.src, name, 'asset');
      if (attrs.poster !== undefined) validateUrl(attrs.poster, name, 'asset');
      if (attrs.srcset && !attrs.srcset.startsWith('data:')) {
        for (const entry of attrs.srcset.split(',')) validateUrl(entry.trim().split(/\s+/u)[0], name, 'srcset asset');
      }
    }
  }
  for (const [name, css] of files) {
    if (!name.endsWith('.css')) continue;
    for (const match of css.matchAll(/url\(\s*['"]?([^'"\s)]+)['"]?\s*\)/gu)) {
      if (!match[1].startsWith('#')) validateUrl(match[1], name, 'CSS asset');
    }
  }
  for (const route of routes) {
    const name = route.path ? `${route.path}/index.html` : 'index.html';
    const nodes = documents.get(name);
    if (!nodes) { errors.push(`Missing guide page ${name}`); continue; }
    const body = nodes.find(({ tag }) => tag === 'main');
    if (body && /\b(?:TODO|TBD|coming soon|under construction)\b/iu.test(text(body.node))) errors.push(`${name}: placeholder content`);
    const title = nodes.find(({ tag }) => tag === 'title');
    if (!title || !text(title.node).includes(route.title)) errors.push(`${name}: incorrect page title`);
    const headings = nodes.filter(({ tag }) => tag === 'h1');
    if (headings.length !== 1 || text(headings[0].node).trim() !== route.title) errors.push(`${name}: expected one H1 matching ${route.title}`);
    const description = nodes.find(({ tag, attrs }) => tag === 'meta' && attrs.name === 'description');
    if (description?.attrs.content !== route.description) errors.push(`${name}: description differs from route inventory`);
    const canonical = nodes.find(({ tag, attrs }) => tag === 'link' && attrs.rel === 'canonical');
    const expected = `${origin}${base}/${route.path ? `${route.path}/` : ''}`;
    if (canonical?.attrs.href !== expected) errors.push(`${name}: expected canonical ${expected}`);
  }
  const expectedHtml = new Set(['404.html', ...routes.map(({ path }) => path ? `${path}/index.html` : 'index.html')]);
  for (const name of documents.keys()) if (!expectedHtml.has(name)) errors.push(`HTML route absent from navigation inventory: ${name}`);
  if (!documents.has('404.html')) errors.push('Missing generated 404.html');
  return errors;
}

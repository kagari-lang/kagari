export const site = 'https://kagari-lang.github.io';
export const base = '/kagari';
export const repository = 'https://github.com/kagari-lang/kagari';

function parts(path) {
  if (typeof path !== 'string' || /[\\?%\s]/u.test(path)) {
    throw new Error(`Expected a repository-relative path, got ${path}`);
  }
  const [pathname, fragment, ...extra] = path.split('#');
  if (extra.length || pathname.split('/').some((part) => part === '.' || part === '..')) {
    throw new Error(`Unsafe link path: ${path}`);
  }
  return [pathname, fragment];
}

/** Link to a guide route, including the project base and optional heading ID. */
export function page(path = '') {
  const [pathname, fragment] = parts(path);
  const route = pathname.replace(/^\/+|\/+$/gu, '');
  if (route && !/^[a-z0-9-]+(?:\/[a-z0-9-]+)*$/u.test(route)) {
    throw new Error(`Invalid guide route: ${path}`);
  }
  return `${base}/${route ? `${route}/` : ''}${fragment === undefined ? '' : `#${fragment}`}`;
}

/** Link to the authoritative main-branch source, never to a Pages-relative file. */
export function source(path) {
  const [pathname, fragment] = parts(path);
  if (!pathname || pathname.startsWith('/') || !/^[\w./-]+$/u.test(pathname)) {
    throw new Error(`Invalid source path: ${path}`);
  }
  return `${repository}/blob/main/${pathname}${fragment === undefined ? '' : `#${fragment}`}`;
}

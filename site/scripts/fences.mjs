import { bundledLanguages, bundledLanguagesAlias } from 'shiki';

export function isFenceLanguage(language) {
  return typeof language === 'string' && (
    language === 'kagari' || language === 'kgr' || language === 'text' ||
    language === 'plaintext' || Object.hasOwn(bundledLanguages, language) || Object.hasOwn(bundledLanguagesAlias, language)
  );
}

/** Astro supplies the parsed Markdown/MDX tree; do not guess fences with a regex. */
export function validateFences() {
  return (tree, file) => {
    function visit(node) {
      if (node.type === 'code' && !isFenceLanguage(node.lang)) {
        file.fail(`Code fences require a known language (got ${node.lang ?? 'none'}).`, node);
      }
      node.children?.forEach(visit);
    }
    visit(tree);
  };
}

import { readFileSync } from 'node:fs';
import Parser from 'tree-sitter';
import Kagari from 'tree-sitter-kagari';

export const languageDirectory = new URL('../../zed/languages/kagari/', import.meta.url);
export function query(name) {
  return new Parser.Query(Kagari, readFileSync(new URL(name, languageDirectory), 'utf8'));
}
export function parse(text) {
  const parser = new Parser();
  parser.setLanguage(Kagari);
  return parser.parse(text);
}
const highlights = query('highlights.scm');
export function captures(text) {
  return highlights.captures(parse(text).rootNode);
}

// With styles for every supported capture, Zed uses the last covering capture in
// the Tree-sitter stream. This does not model a theme omitting a style.
export function effectiveCapture(found, offset) {
  return found.filter(({ node }) => node.startIndex <= offset && offset < node.endIndex).at(-1)?.name;
}

// Mirror SyntaxLayer::override_id at the pinned Zed commit: exclusive endpoints
// unless .inclusive, and the smallest containing range wins.
export function overrideAt(found, offset) {
  let smallest;
  for (const capture of found) {
    const { node, name } = capture;
    const inside = name.endsWith('.inclusive')
      ? node.startIndex <= offset && offset <= node.endIndex
      : node.startIndex < offset && offset < node.endIndex;
    if (inside && (!smallest || node.endIndex - node.startIndex
      < smallest.node.endIndex - smallest.node.startIndex)) smallest = capture;
  }
  return smallest?.name.replace(/\.inclusive$/, '');
}

// Region classification only, not an imitation of Zed's theme-priority algorithm.
// Queries deliberately capture string segments rather than interpolated parents.
export function tokenize(text) {
  return captures(text).filter(({ node }) => node.endIndex > node.startIndex).map(({ name, node }) => ({
    start: node.startIndex, end: node.endIndex, capture: name,
    type: name.startsWith('comment') ? 'comment' : name.startsWith('string') ? 'string' : 'code',
  }));
}

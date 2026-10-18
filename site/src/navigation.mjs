
import { languagePages } from './language-pages.mjs';
import { manualPages } from './manual-pages.mjs';

// Inventory of shipped pages only. Add a route here in the same change as its content.
export const pages = Object.freeze([
  Object.freeze({
    path: '',
    group: 'Start',
    title: 'Kagari',
    description: 'A statically typed, GC-backed scripting language for Rust-hosted applications.',
  }),
  Object.freeze({
    path: 'start/quickstart',
    group: 'Start',
    title: 'Quickstart',
    description: 'Build the source CLI and follow a complete Kagari control-flow example.',
  }),
  ...manualPages.filter(({ group }) => group === 'Start'),
  ...languagePages,
  ...manualPages.filter(({ group }) => group !== 'Start'),
]);

export const sidebar = [...new Set(pages.map(({ group }) => group))].map((label) => ({
  label,
  items: pages.filter((entry) => entry.group === label).map(({ title, path }) => ({
    label: title,
    // Starlight prefixes config links with Astro's base; content uses page() instead.
    link: `/${path ? `${path}/` : ''}`,
  })),
}));

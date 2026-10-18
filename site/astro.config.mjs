import { fileURLToPath } from 'node:url';
import { defineConfig } from 'astro/config';
import { unified } from '@astrojs/markdown-remark';
import starlight from '@astrojs/starlight';
import { base, repository, site } from './src/links.mjs';
import { sidebar } from './src/navigation.mjs';
import { kagariGrammar } from './src/grammar.mjs';
import { validateFences } from './scripts/fences.mjs';

export default defineConfig({
  site,
  base,
  trailingSlash: 'always',
  output: 'static',
  markdown: {
    processor: unified({ remarkPlugins: [validateFences] }),
    shikiConfig: { langs: [kagariGrammar] },
  },
  vite: {
    define: {
      'import.meta.env.KAGARI_REPOSITORY_ROOT': JSON.stringify(fileURLToPath(new URL('../', import.meta.url))),
    },
  },
  integrations: [
    starlight({
      title: 'Kagari',
      description: 'The Kagari language guide for script authors and Rust embedders.',
      social: [{ icon: 'github', label: 'Source on GitHub', href: repository }],
      editLink: { baseUrl: `${repository}/edit/main/site/` },
      sidebar,
      customCss: ['./src/styles/custom.css'],
      expressiveCode: { shiki: { langs: [kagariGrammar] } },
    }),
  ],
});

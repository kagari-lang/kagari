import { readFileSync } from 'node:fs';

export const grammarFile = new URL('../../editors/vscode/syntaxes/kagari.tmLanguage.json', import.meta.url);

// Adapt registration metadata only; lexical rules remain owned by the editor grammar.
export const kagariGrammar = {
  ...JSON.parse(readFileSync(grammarFile, 'utf8')),
  name: 'kagari',
  aliases: ['kgr'],
};

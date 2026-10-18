// Installation, library and reference route metadata.
export const manualPages = Object.freeze([
  Object.freeze({
    path: 'start/installation',
    group: 'Start',
    title: 'Install from source',
    description: 'Build the kg executable from the Kagari checkout and verify it with a small program.',
  }),
  Object.freeze({
    path: 'start/editors',
    group: 'Start',
    title: 'Editor setup',
    description: "Install local Zed or VS Code syntax support and understand the separate analyzer's current limits.",
  }),
  Object.freeze({
    path: 'library/overview',
    group: 'Standard library',
    title: 'Library and prelude',
    description: 'Use the installed foundation, distinguish implicit prelude names from imports, and find current library contracts.',
  }),
  Object.freeze({
    path: 'library/collections',
    group: 'Standard library',
    title: 'Collections and access',
    description: 'Choose arrays or growable storage, expose live read-only views, and use hash keys with explicit equality contracts.',
  }),
  Object.freeze({
    path: 'library/strings-iteration',
    group: 'Standard library',
    title: 'Strings and iteration',
    description: 'Build immutable text, use checked UTF-8 byte ranges, and consume ordinary lazy iterators without assuming Rust borrowing.',
  }),
  Object.freeze({
    path: 'reference/cli',
    group: 'Reference',
    title: 'CLI and diagnostics',
    description: 'Select source or project commands, check and lint without execution, and interpret kg output and exit statuses.',
  }),
  Object.freeze({
    path: 'reference/status',
    group: 'Reference',
    title: 'Status and limits',
    description: 'Understand the current source-build workflow, execution and library boundaries, and tools that are not yet available.',
  }),
  Object.freeze({
    path: 'reference/specifications',
    group: 'Reference',
    title: 'Specifications and sources',
    description: 'Find the authoritative repository rules, current source APIs, and canonical examples without a second copied specification.',
  }),
]);

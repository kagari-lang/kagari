// Language guide inventory; the parent navigation owns integration and ordering.
export const languagePages = Object.freeze([
  Object.freeze({
    path: 'language/variables',
    group: 'Language',
    title: 'Variables and constants',
    description: 'Choose val, var and const, and understand binding scope without freezing shared objects.',
  }),
  Object.freeze({
    path: 'language/values',
    group: 'Language',
    title: 'Types, values and shared identity',
    description: 'Follow scalar copies, GC-managed objects and shared identity through a small program.',
  }),
  Object.freeze({
    path: 'language/control-flow',
    group: 'Language',
    title: 'Operators and control flow',
    description: 'Compute with checked operators, choose branches and follow while, loop and for execution.',
  }),
  Object.freeze({
    path: 'language/functions',
    group: 'Language',
    title: 'Functions and closures',
    description: 'Pass typed arguments, return values and retain lexical state in callable closures.',
  }),
  Object.freeze({
    path: 'language/data-model',
    group: 'Language',
    title: 'Structs, enums and tuples',
    description: 'Model records and alternatives while preserving field writeability and shared-value behavior.',
  }),
  Object.freeze({
    path: 'language/patterns',
    group: 'Language',
    title: 'Patterns and destructuring',
    description: 'Extract fields, select match arms and distinguish irrefutable bindings from conditional patterns.',
  }),
  Object.freeze({
    path: 'language/error-handling',
    group: 'Language',
    title: 'Option, Result and propagation',
    description: 'Represent absence and business errors, propagate with question mark and distinguish traps from values.',
  }),
  Object.freeze({
    path: 'language/traits',
    group: 'Language',
    title: 'Generics, traits and interfaces',
    description: 'Use checked generic bounds, associated outputs and explicit interface types without Rust ownership rules.',
  }),
  Object.freeze({
    path: 'language/modules',
    group: 'Language',
    title: 'Modules and packages',
    description: 'Organize declarations, control visibility and connect source packages through explicit dependencies.',
  }),
  Object.freeze({
    path: 'language/default-updates',
    group: 'Language',
    title: 'Default and struct update',
    description: 'Construct explicit defaults and fresh struct updates while retaining shared inner objects.',
  }),
]);

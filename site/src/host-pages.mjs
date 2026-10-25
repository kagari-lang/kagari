// Embedding guide inventory; navigation places this group before Reference.
export const hostPages = Object.freeze([
  Object.freeze({
    path: 'host/overview',
    group: 'Embedding',
    title: 'First Rust host',
    description: 'Compile a script, prepare and load its artifact, and call a typed public entry from Rust.',
  }),
  Object.freeze({
    path: 'host/native',
    group: 'Embedding',
    title: 'Native functions and types',
    description: 'Declare, bind and install native APIs explicitly, with checked conversions and receiver access.',
  }),
  Object.freeze({
    path: 'host/ownership',
    group: 'Embedding',
    title: 'Ownership and scoped access',
    description: 'Separate script GC values, retained opaque payloads and scoped Rust borrows across calls and reentry.',
  }),
  Object.freeze({
    path: 'host/reload',
    group: 'Embedding',
    title: 'Hot reload',
    description: 'Publish compatible code atomically, preserve static state and retained computations, and select current bodies for ordinary calls.',
  }),
]);

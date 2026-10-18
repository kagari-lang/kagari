import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import { parseDocument } from 'yaml';

const document = parseDocument(await readFile(new URL('../../.github/workflows/pages.yml', import.meta.url), 'utf8'), {
  version: '1.2',
  uniqueKeys: true,
});
assert.deepEqual(document.errors, [], 'Pages workflow must be valid YAML with unique keys');
assert.deepEqual(document.warnings, [], 'Pages workflow must not depend on unsupported YAML features');
const workflow = document.toJS({ maxAliasCount: 0 });
const { build, deploy } = workflow.jobs;

function keys(value, expected) {
  assert.deepEqual(Object.keys(value).sort(), [...expected].sort());
}

function action(step, owner) {
  assert.match(step.uses, new RegExp(`^actions/${owner}@[a-f0-9]{40}$`, 'u'), 'Use a reviewed immutable official action pin');
}

function withoutName(step) {
  assert.equal(typeof step.name, 'string');
  const { name, ...contract } = step;
  return contract;
}

// Interpret only this guard's equality conjunction, not arbitrary JavaScript or a
// substring match that could overlook an appended OR or a disabled requirement.
function deployClauses(condition) {
  const expression = condition.trim().match(/^\$\{\{\s*([\s\S]*?)\s*\}\}$/u);
  assert(expression, 'Use an explicit GitHub expression for the deployment guard');
  return expression[1].split(/\s*&&\s*/u).map((clause) => {
    const comparison = clause.trim().match(/^([\w.]+)\s*(==|!=)\s*'([^']*)'$/u);
    assert(comparison, `Unexpected deployment condition: ${clause}`);
    return comparison.slice(1);
  });
}

function canDeploy(clauses, context) {
  return clauses.every(([path, operator, expected]) => {
    const actual = path.split('.').reduce((value, field) => value?.[field], context) ?? '';
    // These context fields are strings; GitHub Actions compares strings without case.
    // https://docs.github.com/en/actions/reference/workflows-and-actions/expressions#operators
    assert.equal(typeof actual, 'string');
    const equal = actual.toLowerCase() === expected.toLowerCase();
    return operator === '==' ? equal : !equal;
  });
}

test('Pages validates every PR, main push and manual dispatch without path filters', () => {
  assert.deepEqual(workflow.on, {
    pull_request: null,
    push: { branches: ['main'] },
    workflow_dispatch: null,
  });
  // In particular, no workflow_run or pull_request_target privileged handoff.
  keys(workflow, ['name', 'on', 'permissions', 'concurrency', 'jobs']);
  keys(workflow.jobs, ['build', 'deploy']);
});

test('PR build has only read permissions, no secrets, environment or privileged jobs', () => {
  assert.deepEqual(workflow.permissions, { contents: 'read' });
  assert.deepEqual(build.permissions, { contents: 'read' });
  keys(build, ['runs-on', 'timeout-minutes', 'permissions', 'defaults', 'steps']);
  assert.equal(build['runs-on'], 'ubuntu-latest');
  assert.equal(build['timeout-minutes'], 55);
  assert.deepEqual(build.defaults, { run: { 'working-directory': 'site' } });
  assert.doesNotMatch(JSON.stringify(workflow), /secrets\s*[.\[]/u);
  assert.deepEqual(deploy.permissions, { contents: 'read', pages: 'write', 'id-token': 'write' });
});

test('build uses Node 24 and engine-strict locked install with visible high-threshold audit', () => {
  const [checkout, node, install, audit] = build.steps.map(withoutName);
  action(checkout, 'checkout');
  assert.deepEqual(checkout, {
    uses: checkout.uses,
    with: { 'persist-credentials': false },
  });
  action(node, 'setup-node');
  assert.deepEqual(node, {
    uses: node.uses,
    with: { 'node-version': '24', 'package-manager-cache': false },
  });
  assert.deepEqual(install, { 'timeout-minutes': 5, run: 'npm ci --engine-strict' });
  // High is deliberate: moderate upstream findings are reported, not silenced
  // with --omit=dev, || true, continue-on-error or an incompatible override.
  assert.deepEqual(audit, { 'timeout-minutes': 2, run: 'npm audit --audit-level=high' });
});

test('browser tool acquisition is explicit, version-locked and bounded', async () => {
  const [, , , , rust, bindgen] = build.steps.map(withoutName);
  assert.deepEqual(rust, {
    'timeout-minutes': 3,
    'working-directory': '.',
    run: 'rustup toolchain install stable --profile minimal\nrustup default stable\nrustup target add wasm32-unknown-unknown\n',
  });
  assert.deepEqual(bindgen, {
    'timeout-minutes': 15,
    'working-directory': '.',
    run: 'cargo install wasm-bindgen-cli --version 0.2.129 --locked --root target/wasm-tools',
  });
  const lock = await readFile(new URL('../../Cargo.lock', import.meta.url), 'utf8');
  const versions = [...lock.matchAll(/\[\[package\]\]\s+name = "wasm-bindgen"\s+version = "([^"]+)"/gu)].map((match) => match[1]);
  assert.deepEqual(versions, ['0.2.129'], 'Review the CLI acquisition pin with Cargo.lock');
});

test('upload consumes exactly the checked 25-page dist with no rebuild or other build jobs', async () => {
  assert.equal(build.steps.length, 8);
  const [, , , , , , compile, upload] = build.steps.map(withoutName);
  assert.deepEqual(compile, { 'timeout-minutes': 28, run: 'npm run build:pages' });
  const pkg = JSON.parse(await readFile(new URL('../package.json', import.meta.url), 'utf8'));
  assert.equal(pkg.scripts['build:pages'], 'node scripts/build-pages.mjs');
  assert.equal(pkg.scripts.build, 'node scripts/build.mjs');
  assert.equal(pkg.scripts.check, 'node scripts/check.mjs');
  assert.equal(pkg.scripts['prebuild:pages'], undefined);
  assert.equal(pkg.scripts['postbuild:pages'], undefined);
  action(upload, 'upload-pages-artifact');
  assert.deepEqual(upload, {
    uses: upload.uses,
    with: { name: 'github-pages', path: 'site/dist' },
  });
  // Exact step contracts exclude conditionally skipped checks, ignored failures,
  // copying/rebuilding output, alternate sources and unrelated editor workloads.
});

test('deploy requires successful build, main, non-PR event and explicit Actions string opt-in', () => {
  assert.equal(deploy.needs, 'build');
  const clauses = deployClauses(deploy.if);
  assert.deepEqual(clauses, [
    ['needs.build.result', '==', 'success'],
    ['github.ref', '==', 'refs/heads/main'],
    ['github.event_name', '!=', 'pull_request'],
    ['vars.PAGES_DEPLOY_ENABLED', '==', 'true'],
  ]);
  for (const result of ['success', 'failure', 'cancelled', 'skipped']) {
    for (const ref of ['refs/heads/main', 'refs/heads/topic', 'refs/pull/7/merge']) {
      for (const event of ['pull_request', 'push', 'workflow_dispatch']) {
        for (const enabled of [undefined, '', 'false', 'True', 'TRUE', 'true', ' true ', '1']) {
          const context = {
            needs: { build: { result } },
            github: { ref, event_name: event },
            vars: { PAGES_DEPLOY_ENABLED: enabled },
          };
          assert.equal(canDeploy(clauses, context),
            result === 'success' && ref === 'refs/heads/main' && event !== 'pull_request' && ['true', 'True', 'TRUE'].includes(enabled),
            JSON.stringify(context));
        }
      }
    }
  }
});

test('deploy publishes the same run artifact in the Pages environment without executing site code', () => {
  keys(deploy, ['needs', 'if', 'runs-on', 'timeout-minutes', 'permissions', 'environment', 'steps']);
  assert.equal(deploy['runs-on'], 'ubuntu-latest');
  assert.equal(deploy['timeout-minutes'], 15);
  assert.deepEqual(deploy.environment, {
    name: 'github-pages',
    url: '${{ steps.deployment.outputs.page_url }}',
  });
  assert.equal(deploy.steps.length, 2);
  const [freshness, publish] = deploy.steps.map(withoutName);
  action(freshness, 'github-script');
  assert.deepEqual(freshness, { uses: freshness.uses, with: { script: freshness.with.script } });
  keys(freshness.with, ['script']);
  action(publish, 'deploy-pages');
  assert.deepEqual(publish, {
    id: 'deployment',
    uses: publish.uses,
    with: { artifact_name: build.steps.at(-1).with.name },
  });
});

test('main runs cancel superseded work and freshness check rejects old main reruns or API failure', async () => {
  assert.deepEqual(workflow.concurrency, {
    group: 'pages-${{ github.ref }}',
    'cancel-in-progress': true,
  });
  // Execute the small inline action with a read-only API stub. No checkout,
  // credentials, network, artifact mutation or real deployment is involved.
  const AsyncFunction = Object.getPrototypeOf(async () => {}).constructor;
  const check = new AsyncFunction('github', 'context', 'core', deploy.steps[0].with.script);
  for (const head of ['this-revision', 'newer-revision']) {
    const calls = [];
    const failures = [];
    const github = { rest: { git: {
      async getRef(parameters) {
        calls.push(parameters);
        return { data: { object: { sha: head } } };
      },
    } } };
    await check(github, { repo: { owner: 'kagari-lang', repo: 'kagari' }, sha: 'this-revision' }, {
      setFailed(message) { failures.push(message); },
    });
    assert.deepEqual(calls, [{ owner: 'kagari-lang', repo: 'kagari', ref: 'heads/main' }]);
    if (head === 'this-revision') assert.deepEqual(failures, []);
    else {
      assert.equal(failures.length, 1);
      assert.match(failures[0], /superseded/u);
    }
  }
  await assert.rejects(check({ rest: { git: {
    async getRef() { throw new Error('API unavailable'); },
  } } }, { repo: {}, sha: 'this-revision' }, {}), /API unavailable/u);
});

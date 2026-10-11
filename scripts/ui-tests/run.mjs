// Runs the browser tests: starts the release server on a throwaway database and image folder,
// starts a headless browser with WebDriver BiDi, and runs every flow in tests.mjs (or those whose
// name contains the first argument).
//
// Usage: node scripts/ui-tests/run.mjs [name filter]
// The browser is `firefox` unless RTAG_UI_TEST_BROWSER says otherwise, e.g.
// RTAG_UI_TEST_BROWSER="flatpak run app.zen_browser.zen". Build first (scripts/ui-test.sh does).

import { spawn } from 'node:child_process';
import { existsSync, mkdirSync, rmSync } from 'node:fs';
import { createServer } from 'node:net';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { connect } from './bidi.mjs';
import { api, writeTestImage } from './rtag.mjs';
import { LABELS, tests } from './tests.mjs';

const root = join(dirname(fileURLToPath(import.meta.url)), '..', '..');
const work = join(root, 'target', 'ui-tests', `run-${process.pid}`);
const filter = process.argv[2] ?? '';

const freePort = () => new Promise((resolve) => {
  const server = createServer();
  server.listen(0, '127.0.0.1', () => {
    const { port } = server.address();
    server.close(() => resolve(port));
  });
});
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

async function until(check, what, timeout = 30000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    if (await check().catch(() => false)) return;
    await sleep(200);
  }
  throw new Error(`timed out waiting for ${what}`);
}

const server = join(root, 'target', 'release', 'rtag-server');
if (!existsSync(server) || !existsSync(join(root, 'crates', 'rtag-ui', 'dist', 'rtag-ui_bg.wasm'))) {
  console.error('Build first: scripts/ui-test.sh builds the server and the UI and then runs this.');
  process.exit(2);
}
mkdirSync(join(work, 'images'), { recursive: true });
mkdirSync(join(work, 'failures'), { recursive: true });
writeTestImage(join(work, 'images', 'test.png'));

const [appPort, browserPort] = [await freePort(), await freePort()];
const base = `http://127.0.0.1:${appPort}`;
const children = [];
const start = (command, args, env) => {
  // Own process group, so the whole tree (flatpak spawns several) can be stopped at the end.
  const child = spawn(command, args, { env: { ...process.env, ...env }, detached: true, stdio: ['ignore', 'pipe', 'pipe'] });
  children.push(child);
  return child;
};
const stopAll = () => children.forEach((child) => { try { process.kill(-child.pid, 'SIGTERM'); } catch {} });
process.on('exit', stopAll);
process.on('SIGINT', () => process.exit(130));

start(server, [], {
  RTAG_ADDR: `127.0.0.1:${appPort}`,
  RTAG_DATABASE: `sqlite://${join(work, 'rtag.db')}?mode=rwc`,
  RTAG_IMAGES_ROOT: join(work, 'images'),
  RTAG_CACHE_DIR: join(work, 'cache'),
  RTAG_WEB_ROOT: join(root, 'crates', 'rtag-ui', 'dist'),
});
await until(async () => (await fetch(`${base}/api/projects`)).ok, 'the server');

const [command, ...prefix] = (process.env.RTAG_UI_TEST_BROWSER ?? 'firefox').split(' ').filter(Boolean);
// A flatpak browser only sees folders it is given.
if (command === 'flatpak' && prefix[0] === 'run') prefix.splice(1, 0, `--filesystem=${work}`);
const browserProcess = start(command, [...prefix, '--headless', '--no-remote', '--profile', join(work, 'profile'),
  '--remote-debugging-port', String(browserPort), 'about:blank']);
let browserOutput = '';
for (const stream of [browserProcess.stdout, browserProcess.stderr]) stream.on('data', (chunk) => { browserOutput += chunk; });
mkdirSync(join(work, 'profile'), { recursive: true });
await until(async () => browserOutput.includes('WebDriver BiDi listening'), 'the browser');

const b = await connect(browserPort);
const client = api(base);
let failed = 0;
const selected = tests.filter((test) => test.name.includes(filter));
for (const [index, test] of selected.entries()) {
  const started = Date.now();
  try {
    const project = await client.project(`UI test ${index + 1}`, LABELS);
    await test.run({ b, base, api: client, project });
    console.log(`  ✓ ${test.name} (${Date.now() - started} ms)`);
  } catch (err) {
    failed++;
    const shot = join(work, 'failures', `${index + 1}.png`);
    await b.shot(shot).catch(() => {});
    console.log(`  ✗ ${test.name}\n      ${err.message}\n      screenshot: ${shot}`);
  }
}
await b.close();
console.log(`\n${selected.length - failed} passed, ${failed} failed`);
stopAll();
if (failed === 0) rmSync(work, { recursive: true, force: true });
process.exit(failed === 0 ? 0 : 1);

// Minimal WebDriver BiDi client for a headless Firefox-family browser started with
// --remote-debugging-port. Only what the UI tests need: navigation, script evaluation,
// screenshots and real input events.

import { writeFileSync } from 'node:fs';

export const KEY = { ctrl: '', shift: '', alt: '', escape: '', enter: '', delete: '', home: '' };

export async function connect(port) {
  const ws = new WebSocket(`ws://127.0.0.1:${port}/session`);
  let nextId = 1;
  const pending = new Map();
  const logs = [];
  ws.onmessage = (message) => {
    const data = JSON.parse(message.data);
    if (data.id && pending.has(data.id)) {
      const { resolve, reject } = pending.get(data.id);
      pending.delete(data.id);
      if (data.type === 'error') reject(new Error(`${data.error}: ${data.message}`));
      else resolve(data.result);
    } else if (data.method === 'log.entryAdded') {
      logs.push(`[${data.params.level}] ${data.params.text}`);
    }
  };
  await new Promise((resolve, reject) => { ws.onopen = resolve; ws.onerror = reject; });
  const send = (method, params = {}) => new Promise((resolve, reject) => {
    const id = nextId++;
    pending.set(id, { resolve, reject });
    ws.send(JSON.stringify({ id, method, params }));
  });

  await send('session.new', { capabilities: {} });
  await send('session.subscribe', { events: ['log.entryAdded'] });
  const { contexts } = await send('browsingContext.getTree', {});
  const context = contexts[0].context;
  await send('browsingContext.setViewport', { context, viewport: { width: 1400, height: 900 } });

  const pointer = (actions) => ({ type: 'pointer', id: 'mouse', actions });
  const keyboard = (actions) => ({ type: 'key', id: 'kb', actions });
  const round = Math.round;

  const browser = {
    logs,
    sleep: (ms) => new Promise((resolve) => setTimeout(resolve, ms)),
    goto: (url) => send('browsingContext.navigate', { context, url, wait: 'complete' }),
    async eval(expression) {
      const result = await send('script.evaluate', { expression, target: { context }, awaitPromise: true, resultOwnership: 'none' });
      if (result.type === 'exception') throw new Error(`page threw: ${result.exceptionDetails.text}`);
      return result.result.value;
    },
    async json(expression) {
      return JSON.parse(await browser.eval(`JSON.stringify(${expression})`));
    },
    async shot(file) {
      const { data } = await send('browsingContext.captureScreenshot', { context });
      writeFileSync(file, Buffer.from(data, 'base64'));
    },
    actions: (actions) => send('input.performActions', { context, actions }),
    move: (x, y, duration = 0) => browser.actions([pointer([{ type: 'pointerMove', x: round(x), y: round(y), duration }])]),
    /** Presses a key and keeps it down until `keyUp`. */
    keyDown: (value) => browser.actions([keyboard([{ type: 'keyDown', value }])]),
    keyUp: (value) => browser.actions([keyboard([{ type: 'keyUp', value }])]),
    /** Presses and releases one key, optionally with modifiers held. */
    key: (value, modifiers = []) => browser.actions([keyboard([
      ...modifiers.map((m) => ({ type: 'keyDown', value: m })),
      { type: 'keyDown', value }, { type: 'keyUp', value },
      ...modifiers.slice().reverse().map((m) => ({ type: 'keyUp', value: m })),
    ])]),
    /** A click at (x, y) with `button` (0 left, 1 middle, 2 right) and modifiers held. */
    click: (x, y, { button = 0, modifiers = [] } = {}) => browser.actions([
      keyboard([...modifiers.map((m) => ({ type: 'keyDown', value: m })), { type: 'pause' }, { type: 'pause' }, { type: 'pause' },
        ...modifiers.map((m) => ({ type: 'keyUp', value: m }))]),
      pointer([{ type: 'pause' }, { type: 'pointerMove', x: round(x), y: round(y) }, { type: 'pointerDown', button }, { type: 'pointerUp', button }]),
    ]),
    /** Press at `from`, move to `to` in two steps, release; modifiers are held throughout. */
    drag: (from, to, { button = 0, modifiers = [] } = {}) => browser.actions([
      keyboard([...modifiers.map((m) => ({ type: 'keyDown', value: m })), ...Array(5).fill({ type: 'pause' }),
        ...modifiers.map((m) => ({ type: 'keyUp', value: m }))]),
      pointer([
        { type: 'pause' },
        { type: 'pointerMove', x: round(from[0]), y: round(from[1]) }, { type: 'pointerDown', button },
        { type: 'pointerMove', x: round((from[0] + to[0]) / 2), y: round((from[1] + to[1]) / 2), duration: 80 },
        { type: 'pointerMove', x: round(to[0]), y: round(to[1]), duration: 80 }, { type: 'pointerUp', button },
      ]),
    ]),
    /** Holds `key` while the pointer moves through `path`, then releases the key. */
    holdKeyAndMove: (key, path) => browser.actions([
      keyboard([{ type: 'keyDown', value: key }, ...path.map(() => ({ type: 'pause' })), { type: 'pause' }, { type: 'keyUp', value: key }]),
      pointer([{ type: 'pause' }, ...path.map(([x, y]) => ({ type: 'pointerMove', x: round(x), y: round(y), duration: 120 })), { type: 'pause', duration: 50 }]),
    ]),
    /** Polls `check` (async, returns truthy when done) until it passes or `timeout` ms pass. */
    async waitFor(check, description, timeout = 6000) {
      const deadline = Date.now() + timeout;
      let last;
      while (Date.now() < deadline) {
        try {
          last = await check();
          if (last) return last;
        } catch (err) {
          last = err.message;
        }
        await browser.sleep(100);
      }
      throw new Error(`timed out waiting for ${description} (last: ${JSON.stringify(last)})`);
    },
    async close() {
      await send('session.end').catch(() => {});
      ws.close();
    },
  };
  return browser;
}

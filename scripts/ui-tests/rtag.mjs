// rtag-specific helpers for the UI tests: API setup calls, a generated test image, and reading
// the workspace state from the page.

import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname } from 'node:path';
import { crc32, deflateSync } from 'node:zlib';

/** Writes a grayscale PNG with a diagonal gradient and a grid, big enough for two pyramid levels. */
export function writeTestImage(path, width = 1600, height = 1000) {
  const raw = Buffer.alloc((width + 1) * height);
  for (let y = 0; y < height; y++) {
    raw[y * (width + 1)] = 0; // filter: none
    for (let x = 0; x < width; x++) {
      const grid = x % 100 === 0 || y % 100 === 0;
      raw[y * (width + 1) + 1 + x] = grid ? 255 : Math.round(((x + y) / (width + height)) * 200);
    }
  }
  const chunk = (type, data) => {
    const length = Buffer.alloc(4);
    length.writeUInt32BE(data.length);
    const body = Buffer.concat([Buffer.from(type, 'ascii'), data]);
    const crc = Buffer.alloc(4);
    crc.writeUInt32BE(crc32(body));
    return Buffer.concat([length, body, crc]);
  };
  const header = Buffer.alloc(13);
  header.writeUInt32BE(width, 0);
  header.writeUInt32BE(height, 4);
  header[8] = 8; // bit depth
  header[9] = 0; // grayscale
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk('IHDR', header),
    chunk('IDAT', deflateSync(raw)),
    chunk('IEND', Buffer.alloc(0)),
  ]));
}

export function api(base) {
  const call = async (method, path, body) => {
    const response = await fetch(base + path, {
      method,
      headers: { 'content-type': 'application/json' },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
    const text = await response.text();
    if (!response.ok) throw new Error(`${method} ${path} → ${response.status} ${text}`);
    return text ? JSON.parse(text) : null;
  };
  return {
    call,
    /** A project with `labels` ({name, color, hotkey, shapes}) and the test image imported. */
    async project(name, labels) {
      const project = await call('POST', '/api/projects', { name });
      for (const label of labels) {
        await call('POST', `/api/projects/${project.id}/labels`, {
          name: label.name, color: label.color, hotkey: label.hotkey, shape_types: label.shapes ?? ['rectangle'],
        });
      }
      await call('POST', `/api/projects/${project.id}/imports`, { paths: ['test.png'] });
      const page = await call('GET', `/api/projects/${project.id}/images`);
      const image = page.images[0];
      return { id: project.id, image: image.id, annotations: `/api/projects/${project.id}/images/${image.id}/annotations` };
    },
    annotations: (project) => call('GET', project.annotations),
    async setAnnotations(project, annotations) {
      const { version } = await call('GET', project.annotations);
      return call('PUT', project.annotations, { base_version: version, annotations });
    },
  };
}

export const box = (label, x0, y0, x1, y1) => ({ label, shape: { type: 'rectangle', min: { x: x0, y: y0 }, max: { x: x1, y: y1 } } });

/** Nine labels: "line" (3) only allows polylines; the others allow every shape but polylines. */
export const NINE_LABELS = ['crack', 'finger', 'line', 'surface', 'shunt', 'chip', 'dark', 'edge', 'bubble'].map((name, i) => ({
  name,
  color: ['#e74c3c', '#3498db', '#4fb286', '#f1c40f', '#9b59b6', '#e67e22', '#7c8a99', '#1abc9c', '#ff77aa'][i],
  hotkey: String(i + 1),
  shapes: name === 'line' ? ['polyline'] : ['rectangle', 'oriented_rectangle', 'polygon', 'points', 'circle', 'mask'],
}));

/**
 * Opens the workspace for `project` and returns helpers: `at(x, y)` maps image coordinates to
 * screen coordinates for the fitted view, `state()` reads the editor state the page exposes.
 */
export async function openWorkspace(b, base, project, imageSize = [1600, 1000]) {
  await b.goto(`${base}/projects/${project.id}/images/${project.image}`);
  await b.waitFor(() => b.eval(`!!document.querySelector('canvas.annotations') && document.querySelector('button.zoom').textContent.includes('%')`), 'the workspace');
  const canvas = await b.json(`document.querySelector('canvas.annotations').getBoundingClientRect()`);
  // Same as View::fit with its 24 px margin and no enlarging past 100%.
  const scale = Math.min((canvas.width - 48) / imageSize[0], (canvas.height - 48) / imageSize[1], 1);
  const left = canvas.x + (canvas.width - imageSize[0] * scale) / 2;
  const top = canvas.y + (canvas.height - imageSize[1] * scale) / 2;
  const at = (x, y) => [left + x * scale, top + y * scale];
  const state = () => b.json(`(() => {
    const data = document.querySelector('canvas.annotations').dataset;
    const pie = document.querySelector('.pie');
    return {
      count: Number(data.count),
      selected: data.selected === '' ? [] : data.selected.split(',').map(Number),
      tool: document.querySelector('.tool.active')?.getAttribute('aria-label'),
      label: document.querySelector('.label-current')?.innerText.replace(/\\s+/g, ' ').replace(' ▾', '').trim(),
      save: document.querySelector('.save-state')?.textContent,
      pie: pie ? pie.getAttribute('aria-label') : null,
      pieItems: pie ? [...pie.querySelectorAll('.pie-button')].map((x) => ({ name: x.querySelector('.grow').textContent.trim(), key: x.querySelector('.pie-key')?.textContent ?? null, enabled: !x.disabled, current: x.classList.contains('current') })) : [],
      search: !!document.querySelector('.label-search'),
      cursor: document.querySelector('canvas.annotations').style.cursor,
      drawing: Number(data.drawing),
      notice: document.querySelector('.notice')?.textContent ?? null,
    };
  })()`);
  return { at, scale, state, canvas };
}

// The browser-level flows of the annotation workspace. Each test gets a fresh project with the
// nine labels of NINE_LABELS (only "line", 3, rejects rectangles) and one 1600 × 1000 image.

import { writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { KEY } from './bidi.mjs';
import { ADMIN, NINE_LABELS, api, box, openWorkspace, signIn, signOut, writeTestImage } from './rtag.mjs';

function equal(actual, expected, what) {
  const a = JSON.stringify(actual);
  const e = JSON.stringify(expected);
  if (a !== e) throw new Error(`${what}: expected ${e}, got ${a}`);
}

/** Rectangle corners rounded to whole pixels, for comparing with what was drawn on screen. */
const corners = (annotation) => {
  const { min, max } = annotation.shape;
  return [min.x, min.y, max.x, max.y].map(Math.round);
};
const near = (actual, expected, tolerance, what) => {
  if (actual.length !== expected.length || actual.some((v, i) => Math.abs(v - expected[i]) > tolerance)) {
    throw new Error(`${what}: expected about ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`);
  }
};

/** Waits until the server has the annotations `check` accepts (autosave runs about 1 s after an edit). */
const saved = (t, check, what) => t.b.waitFor(async () => {
  const { annotations } = await t.api.annotations(t.project);
  return check(annotations) && annotations;
}, what);

export const tests = [
  {
    name: 'clicking a grid thumbnail opens the image in the workspace',
    async run(t) {
      await t.b.goto(`${t.base}/projects/${t.project.id}`);
      await t.b.waitFor(() => t.b.eval(`!!document.querySelector('.grid li a.open')`), 'the grid');
      const tile = await t.b.json(`document.querySelector('.grid li a.open').getBoundingClientRect()`);
      await t.b.click(tile.x + tile.width / 2, tile.y + tile.height / 2);
      await t.b.waitFor(() => t.b.eval(`!!document.querySelector('canvas.annotations')`), 'the workspace');
      equal(await t.b.eval('location.pathname'), `/projects/${t.project.id}/images/${t.project.image}`, 'address');
    },
  },
  {
    name: 'a dragged rectangle is saved in image coordinates and not selected',
    async run(t) {
      const w = await openWorkspace(t.b, t.base, t.project);
      await t.b.key('r');
      await t.b.drag(w.at(200, 200), w.at(600, 500));
      const [annotation] = await saved(t, (list) => list.length === 1, 'the rectangle to be saved');
      near(corners(annotation), [200, 200, 600, 500], 2 / w.scale, 'rectangle');
      equal(annotation.label, 1, 'label');
      equal((await w.state()).selected, [], 'selection after drawing');
    },
  },
  {
    name: 'two clicks draw a rectangle',
    async run(t) {
      const w = await openWorkspace(t.b, t.base, t.project);
      await t.b.key('r');
      await t.b.click(...w.at(100, 100));
      await t.b.move(...w.at(200, 180), 80);
      await t.b.click(...w.at(300, 250));
      const [annotation] = await saved(t, (list) => list.length === 1, 'the rectangle to be saved');
      near(corners(annotation), [100, 100, 300, 250], 2 / w.scale, 'rectangle');
    },
  },
  {
    name: 'a shape can be selected, moved, resized, undone, redone and deleted',
    async run(t) {
      await t.api.setAnnotations(t.project, [box(1, 200, 200, 400, 400)]);
      const w = await openWorkspace(t.b, t.base, t.project);
      await t.b.click(...w.at(300, 300));
      equal((await w.state()).selected, [0], 'selection after click');
      await t.b.drag(w.at(300, 300), w.at(400, 350));
      let [moved] = await saved(t, (list) => list[0] && corners(list[0])[0] > 250, 'the move to be saved');
      near(corners(moved), [300, 250, 500, 450], 2 / w.scale, 'moved');
      await t.b.drag(w.at(500, 450), w.at(600, 500));
      [moved] = await saved(t, (list) => corners(list[0])[2] > 550, 'the resize to be saved');
      near(corners(moved), [300, 250, 600, 500], 2 / w.scale, 'resized');
      await t.b.key('z', [KEY.ctrl]);
      await saved(t, (list) => Math.abs(corners(list[0])[2] - 500) <= 2 / w.scale, 'the undo to be saved');
      await t.b.key('y', [KEY.ctrl]);
      await saved(t, (list) => Math.abs(corners(list[0])[2] - 600) <= 2 / w.scale, 'the redo to be saved');
      await t.b.click(...w.at(450, 400));
      await t.b.key(KEY.delete);
      await saved(t, (list) => list.length === 0, 'the deletion to be saved');
    },
  },
  {
    name: 'hovering a shape keeps the normal cursor',
    async run(t) {
      await t.api.setAnnotations(t.project, [box(1, 200, 200, 400, 400)]);
      const w = await openWorkspace(t.b, t.base, t.project);
      await t.b.move(...w.at(300, 300), 50);
      const cursor = (await w.state()).cursor;
      if (cursor !== '' && cursor !== 'default') throw new Error(`cursor over a shape: ${cursor}`);
    },
  },
  {
    name: 'Esc leaves a drawing tool for Select',
    async run(t) {
      const w = await openWorkspace(t.b, t.base, t.project);
      await t.b.key('r');
      equal((await w.state()).tool, 'Rectangle', 'tool');
      await t.b.key(KEY.escape);
      equal((await w.state()).tool, 'Select', 'tool after Esc');
    },
  },
  {
    name: 'holding Q and flicking picks a tool and then a label; a tap repeats them',
    async run(t) {
      const w = await openWorkspace(t.b, t.base, t.project);
      const [x, y] = w.at(800, 500);
      await t.b.move(x, y);
      // Flick north-east (Rectangle), keep holding, move south-east in the label pie (surface).
      await t.b.holdKeyAndMove('q', [[x + 120, y - 115], [x + 180, y - 60]]);
      let state = await t.b.waitFor(async () => { const s = await w.state(); return s.pie === null && s; }, 'the pies to close');
      equal([state.tool, state.label], ['Rectangle', 'surface 4'], 'tool and label after the flick pair');
      await t.b.key('v');
      await t.b.move(x, y);
      await t.b.key('q');
      state = await w.state();
      equal([state.tool, state.label, state.pie], ['Rectangle', 'surface 4', null], 'after a tap of Q');
    },
  },
  {
    name: 'a tap of Q repeats the last tool with its mode, also from a drawing tool',
    async run(t) {
      const w = await openWorkspace(t.b, t.base, t.project);
      await t.b.move(...w.at(800, 500));
      await t.b.key('l');
      await clickOption(t, 'Freehand');
      // The options pie names both modes and marks the current one.
      await t.b.keyDown('q');
      equal((await w.state()).pieItems.map((item) => item.name + (item.current ? '*' : '')),
        ['Click', 'Freehand*', 'Snap to edges', 'Finish shape', 'Remove last point'], 'polyline options pie');
      await t.b.key(KEY.escape);
      await t.b.keyUp('q');
      await t.b.key('v');
      await t.b.key('q');
      let state = await w.state();
      equal([state.tool, state.pie, await toolOptions(t)], ['Polyline', null, 'Click | Freehand* | Snap to edgesShift'], 'after a tap from Select');
      await t.b.key('q');
      state = await w.state();
      equal([state.tool, state.pie, await toolOptions(t)], ['Polyline', null, 'Click | Freehand* | Snap to edgesShift'], 'after a tap in Polyline');
    },
  },
  {
    name: 'without labels the drawing tools are disabled and a notice says to create labels',
    async run(t) {
      const bare = await t.api.project('No labels', []);
      const w = await openWorkspace(t.b, t.base, bare);
      let state = await w.state();
      if (!/no labels/i.test(state.notice ?? '')) throw new Error(`notice on opening: ${state.notice}`);
      const [x, y] = w.at(800, 500);
      await t.b.move(x, y);
      await t.b.click(x, y, { button: 2 });
      state = await w.state();
      equal(state.pieItems.filter((item) => item.enabled).map((item) => item.name), ['Select'], 'enabled in the tool pie');
      await t.b.key(KEY.escape);
      equal(await t.b.json(`[...document.querySelectorAll('.tool-rail .tool')].filter((b) => !b.disabled).map((b) => b.getAttribute('aria-label'))`), ['Select'], 'enabled in the rail');
      await t.b.key('r');
      state = await w.state();
      equal(state.tool, 'Select', 'R does nothing');
      if (!/Labels tab/.test(state.notice ?? '')) throw new Error(`notice after R: ${state.notice}`);
      await t.b.key('q', [KEY.shift]);
      equal((await w.state()).pieItems.map((item) => [item.name, item.enabled]), [['No labels yet: add them on the Labels tab', false]], 'label pie');
    },
  },
  {
    name: 'the tool pie opens on a right-button tap and offers all eight tools',
    async run(t) {
      const w = await openWorkspace(t.b, t.base, t.project);
      await t.b.move(...w.at(800, 500));
      await t.b.click(...w.at(800, 500), { button: 2 });
      const state = await w.state();
      equal(state.pie, 'Tools', 'pie after a right-button tap without a last tool');
      equal(state.pieItems.filter((item) => item.enabled).map((item) => item.name), ['Select', 'Rectangle', 'Polygon', 'Points', 'Polyline', 'Brush', 'Ellipse', 'Tag'], 'enabled tools');
      equal(state.pieItems.length, 8, 'tool slots');
    },
  },
  {
    name: 'a drawing tool opens its options pie, and options toggle',
    async run(t) {
      const w = await openWorkspace(t.b, t.base, t.project);
      const [x, y] = w.at(800, 500);
      await t.b.key('r');
      await t.b.move(x, y);
      await t.b.holdKeyAndMove('q', [[x - 60, y]]);
      await t.b.move(x, y);
      // Held, Q opens the options pie (a tap would repeat the last tool).
      await t.b.keyDown('q');
      const state = await w.state();
      equal(state.pie, 'Rectangle options', 'pie');
      equal(state.pieItems.find((item) => item.name.startsWith('Square')).current, true, 'Square after picking it');
      await t.b.key(KEY.escape);
      equal((await w.state()).pie, null, 'pie after Esc');
      await t.b.keyUp('q');
    },
  },
  {
    name: 'the label pie relabels the selected shapes',
    async run(t) {
      await t.api.setAnnotations(t.project, [box(1, 200, 200, 400, 400), box(1, 600, 200, 800, 400)]);
      const w = await openWorkspace(t.b, t.base, t.project);
      await t.b.key('a');
      const [x, y] = w.at(1200, 700);
      await t.b.move(x, y);
      // West is "dark" (7).
      await t.b.holdKeyAndMove('q', [[x - 170, y]]);
      await saved(t, (list) => list.every((a) => a.label === 7), 'the relabelling to be saved');
    },
  },
  {
    name: 'Shift+Q opens the label pie; More… finds a label by typing; unusable labels are disabled',
    async run(t) {
      const w = await openWorkspace(t.b, t.base, t.project);
      await t.b.key('r');
      await t.b.move(...w.at(800, 500));
      await t.b.key('Q', [KEY.shift]);
      const state = await w.state();
      equal(state.pie, 'Labels', 'pie');
      equal(state.pieItems.find((item) => item.name.startsWith('line')).enabled, false, '"line" for rectangles');
      equal([state.pieItems.at(-1).name, state.pieItems.at(-1).key], ['More…', '/'], 'last slot');
      await t.b.key('/');
      await t.b.waitFor(async () => (await w.state()).search, 'the label search');
      for (const ch of 'bub') await t.b.key(ch);
      await t.b.key(KEY.enter);
      equal((await w.state()).label, 'bubble 9', 'label after searching');
    },
  },
  {
    name: 'box selection and bulk moves, deletes and undo',
    async run(t) {
      await t.api.setAnnotations(t.project, [box(1, 100, 100, 200, 200), box(1, 300, 100, 400, 200), box(1, 900, 600, 1000, 700)]);
      const w = await openWorkspace(t.b, t.base, t.project);
      await t.b.drag(w.at(50, 50), w.at(450, 250));
      equal((await w.state()).selected, [0, 1], 'box selection');
      await t.b.click(...w.at(950, 650), { modifiers: [KEY.shift] });
      equal((await w.state()).selected, [0, 1, 2], 'after Shift+click');
      await t.b.click(...w.at(150, 150), { modifiers: [KEY.ctrl] });
      equal((await w.state()).selected, [1, 2], 'after Ctrl+click');
      await t.b.drag(w.at(350, 150), w.at(450, 250));
      await saved(t, (list) => corners(list[1])[0] > 350 && corners(list[2])[0] > 950 && corners(list[0])[0] === 100, 'both selected shapes to move');
      await t.b.key('z', [KEY.ctrl]);
      await saved(t, (list) => corners(list[1])[0] === 300 && corners(list[2])[0] === 900, 'one undo to restore both');
      await t.b.key('a');
      await t.b.key(KEY.delete);
      await saved(t, (list) => list.length === 0, 'deleting everything');
      await t.b.key('a', [KEY.alt]);
      equal((await w.state()).selected, [], 'after Alt+A');
    },
  },
  {
    name: 'an edit after someone else saved shows a conflict and Reload loads their version',
    async run(t) {
      await t.api.setAnnotations(t.project, [box(1, 100, 100, 200, 200)]);
      const w = await openWorkspace(t.b, t.base, t.project);
      await t.api.setAnnotations(t.project, [box(2, 500, 500, 600, 600), box(2, 700, 500, 800, 600)]);
      await t.b.click(...w.at(150, 150));
      await t.b.key(KEY.delete);
      await t.b.waitFor(async () => (await w.state()).save === 'Changed elsewhere', 'the conflict');
      equal((await t.api.annotations(t.project)).annotations.length, 2, 'server annotations kept');
      const reload = await t.b.json(`[...document.querySelectorAll('.actions button')].find((b) => b.textContent === 'Reload').getBoundingClientRect()`);
      await t.b.click(reload.x + reload.width / 2, reload.y + reload.height / 2);
      await t.b.waitFor(async () => { const s = await w.state(); return s.save === 'Saved' && s.count === 2; }, 'the reload');
    },
  },
];

// Vector shapes and vertex editing (ticket 08).
const points = (annotation) => (annotation.shape.points ?? []).map((p) => [Math.round(p.x), Math.round(p.y)]);
const polygon = (label, list) => ({ label, shape: { type: 'polygon', points: list.map(([x, y]) => ({ x, y })) } });

tests.push(
  {
    name: 'a polygon is drawn by clicking vertices and finished with Enter',
    async run(t) {
      const w = await openWorkspace(t.b, t.base, t.project);
      await t.b.key('p');
      for (const [x, y] of [[100, 100], [400, 120], [300, 400]]) await t.b.click(...w.at(x, y));
      equal((await w.state()).drawing, 3, 'vertices placed');
      await t.b.key(KEY.enter);
      const [annotation] = await saved(t, (list) => list.length === 1, 'the polygon to be saved');
      equal(annotation.shape.type, 'polygon', 'type');
      near(points(annotation).flat(), [100, 100, 400, 120, 300, 400], 2 / t.scale(w), 'vertices');
    },
  },
  {
    name: 'a polygon also closes by clicking its first vertex',
    async run(t) {
      const w = await openWorkspace(t.b, t.base, t.project);
      await t.b.key('p');
      for (const [x, y] of [[100, 100], [400, 120], [300, 400], [101, 101]]) await t.b.click(...w.at(x, y));
      const [annotation] = await saved(t, (list) => list.length === 1, 'the closed polygon');
      equal(points(annotation).length, 3, 'vertices (the closing click adds none)');
    },
  },
  {
    name: 'a polyline is finished by a double-click, and Shift snaps it to 15°',
    async run(t) {
      const w = await openWorkspace(t.b, t.base, t.project);
      await t.b.key('l');
      equal((await w.state()).label, 'line 3', 'the polyline tool picks a label that allows polylines');
      await t.b.click(...w.at(100, 300));
      // 10 px off horizontal over 300 px: Shift snaps it flat.
      await t.b.click(...w.at(400, 310), { modifiers: [KEY.shift] });
      const [x, y] = w.at(600, 500);
      await t.b.actions([{ type: 'pointer', id: 'mouse', actions: [
        { type: 'pointerMove', x: Math.round(x), y: Math.round(y) },
        { type: 'pointerDown', button: 0 }, { type: 'pointerUp', button: 0 }, { type: 'pointerDown', button: 0 }, { type: 'pointerUp', button: 0 },
      ] }]);
      const [annotation] = await saved(t, (list) => list.length === 1, 'the polyline to be saved');
      equal(annotation.shape.type, 'polyline', 'type');
      const [first, second, third] = annotation.shape.points;
      // Snapped flat: level with the first point, whatever screen rounding put that at.
      if (Math.abs(second.y - first.y) > 1e-6) throw new Error(`second point not snapped: ${JSON.stringify([first, second])}`);
      equal(annotation.shape.points.length, 3, 'points (the double-click adds one)');
      near([third.x, third.y].map(Math.round), [600, 500], 2 / t.scale(w), 'last point');
    },
  },
  {
    name: 'points are placed by clicks and Backspace removes the last one',
    async run(t) {
      const w = await openWorkspace(t.b, t.base, t.project);
      await t.b.key('.');
      for (const [x, y] of [[100, 100], [200, 150], [300, 200]]) await t.b.click(...w.at(x, y));
      await t.b.key('\uE003');
      equal((await w.state()).drawing, 2, 'points after Backspace');
      await t.b.key(KEY.enter);
      const [annotation] = await saved(t, (list) => list.length === 1, 'the points to be saved');
      equal(annotation.shape.type, 'points', 'type');
      near(points(annotation).flat(), [100, 100, 200, 150], 2 / t.scale(w), 'points');
    },
  },
  {
    name: 'the rectangle options pie switches to 3-point oriented rectangles',
    async run(t) {
      const w = await openWorkspace(t.b, t.base, t.project);
      await t.b.key('r');
      const [x, y] = w.at(800, 500);
      await t.b.move(x, y);
      await t.b.holdKeyAndMove('q', [[x + 70, y]]);
      await t.b.click(...w.at(200, 200));
      await t.b.click(...w.at(500, 200));
      await t.b.click(...w.at(400, 300));
      const [annotation] = await saved(t, (list) => list.length === 1, 'the oriented rectangle to be saved');
      const { type, centre, width, height, angle } = annotation.shape;
      equal(type, 'oriented_rectangle', 'type');
      near([centre.x, centre.y, width, height].map(Math.round), [350, 250, 300, 100], 3 / t.scale(w), 'centre and size');
      if (Math.abs(angle) > 0.02) throw new Error(`angle ${angle}`);
    },
  },
  {
    name: 'polygon vertices can be dragged, inserted and removed',
    async run(t) {
      await t.api.setAnnotations(t.project, [polygon(1, [[100, 100], [500, 100], [500, 400], [100, 400]])]);
      const w = await openWorkspace(t.b, t.base, t.project);
      await t.b.click(...w.at(300, 250));
      await t.b.drag(w.at(500, 400), w.at(600, 500));
      let [annotation] = await saved(t, (list) => points(list[0])[2][0] > 550, 'the dragged vertex');
      near(points(annotation)[2], [600, 500], 2 / t.scale(w), 'dragged vertex');
      // The "+" in the middle of the first edge inserts a vertex.
      await t.b.drag(w.at(300, 100), w.at(300, 50));
      [annotation] = await saved(t, (list) => points(list[0]).length === 5, 'the inserted vertex');
      near(points(annotation)[1], [300, 50], 2 / t.scale(w), 'inserted vertex');
      await t.b.click(...w.at(300, 50), { modifiers: [KEY.ctrl] });
      [annotation] = await saved(t, (list) => points(list[0]).length === 4, 'the removed vertex');
      equal(points(annotation)[1], [500, 100], 'vertex after the removed one');
    },
  },
  {
    name: 'an oriented rectangle turns with its rotation handle',
    async run(t) {
      const shape = { type: 'oriented_rectangle', centre: { x: 800, y: 500 }, width: 300, height: 100, angle: 0 };
      await t.api.setAnnotations(t.project, [{ label: 1, shape }]);
      const w = await openWorkspace(t.b, t.base, t.project);
      await t.b.click(...w.at(800, 500));
      // Unrotated, the handle is above the top edge, 24 screen px out.
      const [cx, cy] = w.at(800, 500);
      const top = w.at(800, 450)[1] - 24;
      await t.b.drag([cx, top], [cx + 200, cy]);
      const [annotation] = await saved(t, (list) => Math.abs(list[0].shape.angle) > 1, 'the rotation');
      if (Math.abs(annotation.shape.angle - Math.PI / 2) > 0.05) throw new Error(`angle ${annotation.shape.angle}`);
    },
  },
  {
    name: 'labels restricted to other shapes cannot be used',
    async run(t) {
      const w = await openWorkspace(t.b, t.base, t.project);
      await t.b.key('p');
      await t.b.key('3');
      const state = await w.state();
      equal(state.label, 'crack 1', 'label stays');
      if (!state.notice?.includes('cannot be used for polygon')) throw new Error(`notice: ${state.notice}`);
    },
  },
);

// Freehand polylines, single points, ellipses and the tool settings bar.
const clickOption = async (t, text) => {
  const rect = await t.b.json(`[...document.querySelectorAll('.tool-options button')].find((b) => b.textContent.startsWith(${JSON.stringify(text)})).getBoundingClientRect()`);
  await t.b.click(rect.x + rect.width / 2, rect.y + rect.height / 2);
};
const toolOptions = (t) => t.b.eval(`[...document.querySelectorAll('.tool-options button')].map((b) => b.textContent.trim().replace(/\\s+/g, ' ') + (b.classList.contains('active') ? '*' : '')).join(' | ')`);

tests.push(
  {
    name: 'the tool settings bar shows each tool\'s options',
    async run(t) {
      await openWorkspace(t.b, t.base, t.project);
      const seen = {};
      for (const key of ['r', 'p', 'l', '.', 'c', 'b']) {
        await t.b.key(key);
        seen[key] = await toolOptions(t);
      }
      equal(seen, {
        r: '2-point* | 3-point oriented | From centerAlt | SquareShift',
        p: 'Snap 15°Shift',
        l: 'Click* | Freehand | Snap 15°Shift',
        '.': 'Multiple* | Single point',
        c: 'From center* | Across the box',
        b: 'Paint* | Fuzzy fill | Brush* | Eraser | Stop at edgesShift | Filtered view',
      }, 'options per tool');
    },
  },
  {
    name: 'a freehand polyline places vertices at the chosen spacing while dragging',
    async run(t) {
      const w = await openWorkspace(t.b, t.base, t.project);
      await t.b.key('l');
      await clickOption(t, 'Freehand');
      // ] then [ cancel out; set the spacing in the field instead.
      await t.b.key(']');
      await t.b.key('[');
      const field = await t.b.json(`document.querySelector('.option-number input').getBoundingClientRect()`);
      await t.b.click(field.x + field.width / 2, field.y + field.height / 2);
      await t.b.key('a', [KEY.ctrl]);
      for (const ch of '50') await t.b.key(ch);
      await t.b.key(KEY.enter);
      const path = [];
      for (let x = 100; x <= 400; x += 25) path.push(w.at(x, 300));
      await t.b.actions([{ type: 'pointer', id: 'mouse', actions: [
        { type: 'pointerMove', x: Math.round(path[0][0]), y: Math.round(path[0][1]) }, { type: 'pointerDown', button: 0 },
        ...path.slice(1).map(([x, y]) => ({ type: 'pointerMove', x: Math.round(x), y: Math.round(y), duration: 30 })),
        { type: 'pointerUp', button: 0 },
      ] }]);
      const [annotation] = await saved(t, (list) => list.length === 1, 'the freehand polyline');
      equal(annotation.shape.type, 'polyline', 'type');
      const xs = annotation.shape.points.map((p) => p.x);
      const gaps = xs.slice(1).map((x, i) => Math.round(x - xs[i]));
      if (gaps.length < 5 || gaps.some((gap) => Math.abs(gap - 50) > 1)) throw new Error(`gaps ${JSON.stringify(gaps)}`);
    },
  },
  {
    name: 'single-point mode makes each click its own point annotation',
    async run(t) {
      const w = await openWorkspace(t.b, t.base, t.project);
      await t.b.key('.');
      await clickOption(t, 'Single point');
      await t.b.click(...w.at(100, 100));
      await t.b.click(...w.at(300, 200));
      const list = await saved(t, (list) => list.length === 2, 'two point annotations');
      equal(list.map((a) => [a.shape.type, a.shape.points.length]), [['points', 1], ['points', 1]], 'annotations');
    },
  },
  {
    name: 'ellipses are drawn from the centre or across their box, Shift makes a circle, and handles resize and turn them',
    async run(t) {
      const w = await openWorkspace(t.b, t.base, t.project);
      const tolerance = 2 / t.scale(w);
      const sizes = (shape) => [shape.centre.x, shape.centre.y, shape.rx, shape.ry].map(Math.round);
      await t.b.key('c');
      await t.b.drag(w.at(500, 500), w.at(600, 550));
      let list = await saved(t, (list) => list.length === 1, 'the ellipse');
      equal(list[0].shape.type, 'ellipse', 'type');
      near(sizes(list[0].shape), [500, 500, 100, 50], tolerance, 'from the centre');
      await t.b.drag(w.at(300, 800), w.at(360, 780), { modifiers: [KEY.shift] });
      list = await saved(t, (list) => list.length === 2, 'the circle');
      near(sizes(list[1].shape), [300, 800, 60, 60], tolerance, 'Shift: a circle');
      // Near the left edge the circle shrinks around its centre and stays a circle.
      await t.b.drag(w.at(30, 500), w.at(90, 520), { modifiers: [KEY.shift] });
      list = await saved(t, (list) => list.length === 3, 'the circle at the edge');
      near(sizes(list[2].shape), [30, 500, 30, 30], tolerance, 'a circle at the edge');
      await clickOption(t, 'Across the box');
      await t.b.click(...w.at(200, 200));
      await t.b.click(...w.at(400, 300));
      list = await saved(t, (list) => list.length === 4, 'the boxed ellipse');
      near(sizes(list[3].shape), [300, 250, 100, 50], tolerance, 'across the box');

      await t.b.key('v');
      await t.b.click(...w.at(500, 500));
      await t.b.drag(w.at(600, 500), w.at(650, 500));
      list = await saved(t, (list) => Math.round(list[0].shape.rx) >= 148, 'the rx change');
      await t.b.drag(w.at(500, 550), w.at(500, 580));
      list = await saved(t, (list) => Math.round(list[0].shape.ry) >= 78, 'the ry change');
      near(sizes(list[0].shape), [500, 500, 150, 80], tolerance, 'radii');
      // The rotation handle sits above the ellipse; pulled to its right, it turns it a quarter.
      const [hx, hy] = w.at(500, 500 - 80);
      await t.b.drag([hx, hy - 24], w.at(700, 500));
      list = await saved(t, (list) => Math.abs(list[0].shape.angle - Math.PI / 2) < 0.05, 'the turn');
      near(sizes(list[0].shape), [500, 500, 150, 80], tolerance, 'turned, same size');
    },
  },
);

// Masks (ticket 09).

/** Whether image pixel (x, y) is set in a stored mask (row-major runs, starting unset). */
const maskHas = (shape, x, y) => {
  if (x < shape.left || y < shape.top || x >= shape.left + shape.width || y >= shape.top + shape.height) return false;
  const index = (y - shape.top) * shape.width + (x - shape.left);
  let end = 0;
  for (let k = 0; k < shape.rle.length; k++) {
    end += shape.rle[k];
    if (index < end) return k % 2 === 1;
  }
  return false;
};
const setPixels = (shape) => shape.rle.filter((_, i) => i % 2 === 1).reduce((a, b) => a + b, 0);
const setThreshold = async (t, value) => {
  const field = await t.b.json(`document.querySelector('.option-number input').getBoundingClientRect()`);
  await t.b.click(field.x + field.width / 2, field.y + field.height / 2);
  await t.b.key('a', [KEY.ctrl]);
  for (const ch of String(value)) await t.b.key(ch);
  await t.b.key(KEY.enter);
};
const sizeField = (t) => t.b.eval(`document.querySelector('.option-number input').value`);

tests.push({
  name: 'the brush paints masks, strokes that touch a mask extend it, and the eraser removes pixels',
  async run(t) {
    const w = await openWorkspace(t.b, t.base, t.project);
    await t.b.key('b');
    equal(await sizeField(t), '20', 'default brush size');
    await t.b.drag(w.at(200, 300), w.at(400, 300));
    let list = await saved(t, (list) => list.length === 1, 'the first stroke');
    let [mask] = list.map((a) => a.shape);
    equal([mask.type, list[0].label], ['mask', 1], 'type and label');
    near([mask.left, mask.top, mask.left + mask.width, mask.top + mask.height], [190, 290, 410, 310], 1 + 2 / w.scale, 'stroke bounds');
    equal([maskHas(mask, 300, 300), maskHas(mask, 300, 330)], [true, false], 'pixels on and beside the stroke');

    // A stroke starting on the mask extends it; one elsewhere is a new mask.
    await t.b.drag(w.at(400, 300), w.at(400, 500));
    list = await saved(t, (list) => list[0].shape.height > 100, 'the extending stroke');
    equal(list.length, 1, 'still one mask');
    await t.b.drag(w.at(1000, 300), w.at(1100, 300));
    list = await saved(t, (list) => list.length === 2, 'a separate mask');

    // E erases across every mask; ] makes the eraser bigger, so the second mask goes entirely.
    await t.b.key('e');
    equal(await toolOptions(t), 'Paint* | Fuzzy fill | Brush | Eraser* | Stop at edgesShift | Filtered view', 'eraser mode');
    await t.b.drag(w.at(300, 250), w.at(300, 350));
    list = await saved(t, (list) => !maskHas(list[0].shape, 300, 300), 'the erased gap');
    equal(maskHas(list[0].shape, 250, 300), true, 'pixels beside the gap stay');
    for (let i = 0; i < 3; i++) await t.b.key(']');
    equal(await sizeField(t), '39', 'size after three ]');
    await t.b.drag(w.at(990, 300), w.at(1110, 300));
    await saved(t, (list) => list.length === 1, 'the emptied mask removed');

    // Each stroke is one undo step.
    await t.b.key('z', [KEY.ctrl]);
    await saved(t, (list) => list.length === 2, 'the removal undone');
    await t.b.key('z', [KEY.ctrl]);
    await saved(t, (list) => maskHas(list[0].shape, 300, 300), 'the gap undone');
    await t.b.key('y', [KEY.ctrl]);
    await saved(t, (list) => !maskHas(list[0].shape, 300, 300), 'the gap redone');
  },
}, {
  name: 'a stroke joining two masks merges them, other labels overlap, and the eraser cuts every label',
  async run(t) {
    const square = (label, left) => ({ label, shape: { type: 'mask', left, top: 100, width: 100, height: 100, rle: [0, 10000] } });
    // crack (1) at x 100–200 and 400–500, finger (2) between them.
    await t.api.setAnnotations(t.project, [square(1, 100), square(1, 400), square(2, 250)]);
    const w = await openWorkspace(t.b, t.base, t.project);
    await t.b.key('b');
    await t.b.drag(w.at(150, 150), w.at(450, 150));
    let list = await saved(t, (list) => list.length === 2, 'the two crack masks merged');
    equal(list.map((a) => [a.label, a.shape.left, a.shape.width]), [[1, 100, 400], [2, 250, 100]], 'merged crack and untouched finger');
    equal(list[1].shape.rle, [0, 10000], 'the finger mask keeps every pixel');
    await t.b.key('e');
    await t.b.drag(w.at(120, 180), w.at(480, 180));
    list = await saved(t, (list) => !maskHas(list[1].shape, 300, 180), 'the erased line');
    equal([maskHas(list[0].shape, 150, 180), maskHas(list[0].shape, 150, 120)], [false, true], 'crack cut below, kept above');
  },
}, {
  name: 'Ctrl inverts the brush, and F resizes it by moving the pointer',
  async run(t) {
    await t.api.setAnnotations(t.project, [{ label: 1, shape: { type: 'mask', left: 100, top: 100, width: 200, height: 200, rle: [0, 40000] } }]);
    const w = await openWorkspace(t.b, t.base, t.project);
    await t.b.key('b');
    await t.b.drag(w.at(120, 200), w.at(280, 200), { modifiers: [KEY.ctrl] });
    let list = await saved(t, (list) => !maskHas(list[0].shape, 200, 200), 'Ctrl+drag with the brush erasing');
    equal(maskHas(list[0].shape, 200, 120), true, 'pixels away from the stroke stay');
    await t.b.key('e');
    await t.b.drag(w.at(500, 500), w.at(600, 500), { modifiers: [KEY.ctrl] });
    list = await saved(t, (list) => list.length === 2, 'Ctrl+drag with the eraser painting');

    // F: the circle stays where it is; moving right by 50 grows it by 100, a click confirms, Esc cancels.
    await t.b.move(...w.at(800, 500));
    await t.b.key('f');
    await t.b.move(...w.at(850, 500));
    await t.b.click(...w.at(850, 500));
    near([Number(await sizeField(t))], [120], 3, 'size after F, move and click');
    equal((await w.state()).count, 2, 'the confirming click paints nothing');
    await t.b.key('f');
    await t.b.move(...w.at(900, 500));
    await t.b.key(KEY.escape);
    near([Number(await sizeField(t))], [120], 3, 'size after Esc');
  },
}, {
  name: 'the fuzzy fill adds the similar region around a click, across tiles, and a drag sets its threshold',
  async run(t) {
    const w = await openWorkspace(t.b, t.base, t.project);
    await t.b.key('u');
    equal(await toolOptions(t), 'Paint | Fuzzy fill* | Brush* | Eraser | Contiguous* | Global | Filtered view', 'fuzzy fill options');
    // The options pie raises the threshold ( ] picks its item); [ lowers it again.
    await t.b.move(...w.at(800, 500));
    await t.b.keyDown('q');
    equal((await w.state()).pieItems.map((item) => item.name + (item.current ? '*' : '')),
      ['Paint', 'Fuzzy fill*', 'Brush*', 'Eraser', 'Higher threshold', 'Lower threshold'], 'fill options pie');
    await t.b.key(']');
    await t.b.keyUp('q');
    equal(await t.b.eval(`document.querySelector('.option-number input').value`), '20', 'threshold after the pie');
    await t.b.key('[');
    // The test image's grid lines (255) bound each 100 px cell, whose gradient stays within 15.
    await t.b.click(...w.at(550, 350));
    let list = await saved(t, (list) => list.length === 1, 'the filled cell');
    let mask = list[0].shape;
    near([mask.left, mask.top, mask.width, mask.height], [501, 301, 99, 99], 1, 'the cell, across the tile edge at x 512');
    equal(setPixels(mask), mask.width * mask.height, 'every pixel of the cell');

    // Dragging left lowers the threshold to 0: only pixels of the clicked value.
    const [x, y] = w.at(250, 350);
    await t.b.drag([x, y], [x - 40, y]);
    list = await saved(t, (list) => list.length === 2, 'the narrow fill');
    equal(await t.b.eval(`document.querySelector('.option-number input').value`), '0', 'threshold after the drag');
    if (setPixels(list[1].shape) > 3000) throw new Error(`threshold 0 filled ${setPixels(list[1].shape)} pixels`);

    // Global: similar pixels of every cell on screen, not only the clicked one.
    await clickOption(t, 'Global');
    await t.b.key('z', [KEY.ctrl]);
    await setThreshold(t, 15);
    await t.b.click(...w.at(250, 350));
    list = await saved(t, (list) => list.length === 2 && list[1].shape.width > 200, 'the global fill');

    // In eraser mode the region is cut from the masks.
    await clickOption(t, 'Contiguous');
    await t.b.key('e');
    await t.b.click(...w.at(550, 350));
    await saved(t, (list) => list.length === 1, 'the erased cell');

    // A fill running past the maximum size (25% of the image) is not added.
    await t.b.key('u');
    await setThreshold(t, 255);
    await t.b.click(...w.at(550, 350));
    await t.b.waitFor(async () => ((await w.state()).notice ?? '').startsWith('Not added'), 'the runaway notice');
    equal((await t.api.annotations(t.project)).annotations.length, 1, 'annotations after a runaway fill');
  },
}, {
  name: 'freehand lines snap to edges, and Shift draws them freely',
  async run(t) {
    const w = await openWorkspace(t.b, t.base, t.project);
    await t.b.key('l');
    await clickOption(t, 'Freehand');
    await clickOption(t, 'Snap to edges');
    await t.b.sleep(1000); // the analysis tiles of the view load
    // Along the grid line y = 300, 5 px below it: the vertices land on the line.
    const along = (y) => Array.from({ length: 13 }, (_, i) => w.at(120 + i * 25, y));
    const draw = (path, modifiers = []) => t.b.actions([
      { type: 'key', id: 'kb', actions: [...modifiers.map((m) => ({ type: 'keyDown', value: m })), ...path.map(() => ({ type: 'pause' })), { type: 'pause' }, { type: 'pause' }, ...modifiers.map((m) => ({ type: 'keyUp', value: m }))] },
      { type: 'pointer', id: 'mouse', actions: [{ type: 'pause' },
        { type: 'pointerMove', x: Math.round(path[0][0]), y: Math.round(path[0][1]) }, { type: 'pointerDown', button: 0 },
        ...path.slice(1).map(([x, y]) => ({ type: 'pointerMove', x: Math.round(x), y: Math.round(y), duration: 30 })),
        { type: 'pointerUp', button: 0 }] },
    ]);
    await draw(along(305));
    let list = await saved(t, (list) => list.length === 1, 'the snapped line');
    const ys = list[0].shape.points.map((p) => p.y);
    if (ys.length < 5 || ys.some((y) => Math.abs(y - 300) > 2.5)) throw new Error(`snapped ys ${JSON.stringify(ys.map(Math.round))}`);
    await draw(along(705), [KEY.shift]);
    list = await saved(t, (list) => list.length === 2, 'the free line');
    const free = list[1].shape.points.map((p) => p.y);
    if (free.some((y) => Math.abs(y - 705) > 2)) throw new Error(`free ys ${JSON.stringify(free.map(Math.round))}`);
  },
}, {
  name: 'the brush stops at edges, with Shift painting past them',
  async run(t) {
    const w = await openWorkspace(t.b, t.base, t.project);
    await t.b.key('b');
    await clickOption(t, 'Stop at edges');
    await t.b.sleep(1000);
    // From inside the cell x 501–599 across its right-hand grid line.
    await t.b.drag(w.at(550, 350), w.at(650, 350));
    let list = await saved(t, (list) => list.length === 1, 'the limited stroke');
    let mask = list[0].shape;
    if (mask.left + mask.width > 600 || mask.left < 501) throw new Error(`the stroke left the cell: ${mask.left}..${mask.left + mask.width}`);
    await t.b.drag(w.at(250, 650), w.at(350, 650), { modifiers: [KEY.shift] });
    list = await saved(t, (list) => list.length === 2, 'the free stroke');
    mask = list[1].shape;
    if (mask.left + mask.width < 355) throw new Error(`the Shift stroke stopped at ${mask.left + mask.width}`);
  },
}, {
  name: 'a mask is selected by its pixels and moves by whole pixels',
  async run(t) {
    const mask = { type: 'mask', left: 100, top: 100, width: 200, height: 100, rle: [0, 200 * 50, 200 * 50] };
    await t.api.setAnnotations(t.project, [{ label: 1, shape: mask }]);
    const w = await openWorkspace(t.b, t.base, t.project);
    await t.b.click(...w.at(200, 175));
    equal((await w.state()).selected, [], 'a click on the unset half');
    await t.b.click(...w.at(200, 125));
    equal((await w.state()).selected, [0], 'a click on the set half');
    await t.b.drag(w.at(200, 125), w.at(300, 175));
    const [moved] = await saved(t, (list) => list[0].shape.left !== 100, 'the move');
    near([moved.shape.left, moved.shape.top], [200, 150], 1 + 2 / w.scale, 'new position');
    equal([moved.shape.width, moved.shape.height, moved.shape.rle], [200, 100, mask.rle], 'same pixels');
  },
});

// Tags and attributes (ticket 10).

/** Gives label `id` of the test project `attributes`, keeping the rest of its definition. */
async function setAttributes(t, id, attributes) {
  const labels = await t.api.call('GET', `/api/projects/${t.project.id}/labels`);
  const label = labels.find((l) => l.id === id);
  await t.api.call('PUT', `/api/projects/${t.project.id}/labels/${id}`, { ...label, attributes });
}
const chips = (t) => t.b.json(`[...document.querySelectorAll('.tag-chip')].map((c) => c.querySelector('.name').textContent)`);
const panelTags = (t) => t.b.json(`[...document.querySelectorAll('.side-panel .tags li')].map((c) => c.querySelector('.name').textContent)`);

tests.push({
  name: 'labels define attributes with types and defaults on the Labels tab',
  async run(t) {
    await t.b.goto(`${t.base}/projects/${t.project.id}/labels`);
    await t.b.waitFor(() => t.b.eval(`!!document.querySelector('.label-form')`), 'the label form');
    // Typed like a person: click into the field, type, Tab out.
    const type = async (selector, text) => {
      const box = await t.b.json(`(() => { const el = document.querySelector(${JSON.stringify(selector)}); el.scrollIntoView({ block: 'center' }); return el.getBoundingClientRect(); })()`);
      await t.b.click(box.x + box.width / 2, box.y + box.height / 2);
      for (const ch of text) await t.b.key(ch);
      await t.b.key(KEY.tab);
    };
    const press = (text) => t.b.eval(`[...document.querySelectorAll('.label-form button')].find((b) => b.textContent.trim() === ${JSON.stringify(text)}).click(), 1`);
    const choose = (selector, value) => t.b.eval(`(() => { const el = document.querySelector(${JSON.stringify(selector)}); el.value = ${JSON.stringify(value)}; el.dispatchEvent(new Event('change', { bubbles: true })); return 1; })()`);
    await type('.label-form input[placeholder="Name"]', 'stain');
    await press('Add attribute');
    await type('.attribute:nth-of-type(1) input.attribute-name', 'severity');
    await choose('.attribute:nth-of-type(1) select.attribute-type', 'select');
    await type('.attribute:nth-of-type(1) input.options', 'low, high');
    await choose('.attribute:nth-of-type(1) select.default', 'high');
    await press('Add attribute');
    await type('.attribute:nth-of-type(2) input.attribute-name', 'occluded');
    await choose('.attribute:nth-of-type(2) select.attribute-type', 'checkbox');
    // Replace the default 7 with 4.
    await t.b.eval(`(() => { const el = document.querySelector('.label-form input.line-width'); el.focus(); el.select(); return 1; })()`);
    await t.b.key('4');
    await t.b.key(KEY.tab);
    await press('Add label');
    const label = await t.b.waitFor(async () => (await t.api.call('GET', `/api/projects/${t.project.id}/labels`)).find((l) => l.name === 'stain'), 'the new label');
    equal([label.line_width, label.point_radius], [4, 8], 'line width set, point radius default');
    equal(label.attributes, [
      { name: 'severity', type: 'select', options: ['low', 'high'], default: 'high' },
      { name: 'occluded', type: 'checkbox', default: false },
    ], 'attributes');
  },
}, {
  name: 'T and a label key toggle an image tag, shown over the image and in the side panel',
  async run(t) {
    const w = await openWorkspace(t.b, t.base, t.project);
    await t.b.move(...w.at(800, 500));
    await t.b.key('t');
    equal((await w.state()).pie, 'Labels', 'T opens the label pie');
    await t.b.key('2');
    let list = await saved(t, (list) => list.length === 1, 'the tag');
    equal(list[0], { label: 2, shape: { type: 'tag' } }, 'a tag for "finger"');
    equal([await chips(t), await panelTags(t), (await w.state()).tool], [['finger'], ['finger'], 'Select'], 'chips, panel and tool');
    await t.b.key('t');
    await t.b.key('2');
    await saved(t, (list) => list.length === 0, 'the tag removed again');
    equal(await chips(t), [], 'no chips');
  },
}, {
  name: 'the attribute editor appears only for a selection whose label has attributes',
  async run(t) {
    await setAttributes(t, 1, [
      { name: 'severity', type: 'select', options: ['low', 'high'], default: 'low' },
      { name: 'occluded', type: 'checkbox', default: false },
    ]);
    await t.api.setAnnotations(t.project, [box(1, 100, 100, 300, 300), box(2, 500, 100, 700, 300), { label: 1, shape: { type: 'tag' } }]);
    const w = await openWorkspace(t.b, t.base, t.project);
    const editor = () => t.b.eval(`!!document.querySelector('.attribute-editor')`);
    await t.b.click(...w.at(600, 200));
    equal(await editor(), false, 'no editor for a label without attributes');
    await t.b.click(...w.at(200, 200));
    equal(await editor(), true, 'an editor for "crack"');
    await t.b.eval(`(() => { const el = document.querySelector('.attribute-editor select'); el.value = 'high'; el.dispatchEvent(new Event('change', { bubbles: true })); return 1; })()`);
    await t.b.eval(`document.querySelector('.attribute-editor input[type=checkbox]').click(), 1`);
    let list = await saved(t, (list) => list[0].attributes?.occluded === true, 'the attribute values');
    equal([list[0].attributes.severity, list[0].attributes.occluded], ['high', true], 'values');
    // A tag is selected from its chip, and has the same editor.
    await t.b.eval(`document.querySelector('.tag-chip .name').click(), 1`);
    equal([(await w.state()).selected, await editor()], [[2], true], 'tag selected with an editor');
  },
});

// Objects list, filmstrip and status (ticket 11).
const objectRows = (t) => t.b.json(`[...document.querySelectorAll('.objects li')].map((li) => ({
  label: li.querySelector('select.relabel').selectedOptions[0].textContent, detail: li.querySelector('.detail').textContent,
  hidden: li.classList.contains('is-hidden'), locked: li.classList.contains('is-locked'), selected: li.classList.contains('selected'),
  shown: li.getClientRects().length > 0 }))`);
const clickIn = (t, selector) => t.b.eval(`document.querySelector(${JSON.stringify(selector)}).click(), 1`);

tests.push({
  name: 'the objects list selects, hides, locks and relabels annotations; locked shapes stay put',
  async run(t) {
    await t.api.setAnnotations(t.project, [box(1, 100, 100, 300, 300), box(2, 500, 100, 700, 300)]);
    const w = await openWorkspace(t.b, t.base, t.project);
    let rows = await objectRows(t);
    equal(rows.map((r) => [r.label, r.detail]), [['crack', 'rectangle 200 × 200'], ['finger', 'rectangle 200 × 200']], 'rows');
    await clickIn(t, '.objects li:nth-child(2) .pick');
    equal((await w.state()).selected, [1], 'selected from the list');

    await clickIn(t, '.objects li:nth-child(1) button.hide');
    let list = await saved(t, (list) => list[0].hidden === true, 'the hidden flag');
    await t.b.click(...w.at(200, 200));
    equal((await w.state()).selected, [], 'a hidden shape is not picked');
    await t.b.key('a');
    equal((await w.state()).selected, [1], 'A selects the visible shapes only');
    await t.b.key(KEY.escape);

    await clickIn(t, '.objects li:nth-child(2) button.lock');
    await saved(t, (list) => list[1].locked === true, 'the locked flag');
    await t.b.click(...w.at(600, 200));
    await t.b.drag(w.at(600, 200), w.at(650, 260));
    await t.b.key(KEY.delete);
    await t.b.sleep(1500);
    list = (await t.api.annotations(t.project)).annotations;
    equal([list.length, corners(list[1])], [2, [500, 100, 700, 300]], 'the locked shape neither moved nor went');
    if (!/locked/i.test((await w.state()).notice ?? '')) throw new Error(`notice: ${(await w.state()).notice}`);

    await t.b.eval(`(() => { const s = document.querySelector('.objects li:nth-child(2) select.relabel'); s.value = '4'; s.dispatchEvent(new Event('change', { bubbles: true })); return 1; })()`);
    await saved(t, (list) => list[1].label === 4, 'the relabel');
    rows = await objectRows(t);
    equal(rows.map((r) => [r.label, r.hidden, r.locked, r.shown]), [['crack', true, false, true], ['surface', false, true, true]], 'rows after (a hidden shape stays in the list)');
  },
}, {
  name: 'the filmstrip and j, k, gg and G move through the images; Ctrl+Enter marks one annotated and opens the next',
  async run(t) {
    // Different sizes, so their contents differ (duplicates are skipped on import).
    ['a', 'b', 'c', 'd', 'e'].forEach((name, i) => writeTestImage(join(t.work, 'images', 'strip', `${name}.png`), 120 + i, 80));
    await t.api.call('POST', `/api/projects/${t.project.id}/imports`, { paths: ['strip'] });
    const ids = await t.api.call('GET', `/api/projects/${t.project.id}/image-ids`);
    equal(ids.length, 6, 'images (strip/a–e and test.png)');
    const at = async () => Number(/\/images\/(\d+)$/.exec(await t.b.eval('location.pathname'))?.[1]);
    const waitAt = (index) => t.b.waitFor(async () => (await at()) === ids[index] && (await t.b.eval(`document.querySelector('.image-position')?.textContent`)) === `${index + 1} / 6`, `image ${index + 1}`);
    await openWorkspace(t.b, t.base, { ...t.project, image: ids[0] }, [120, 80]);
    await waitAt(0);
    await t.b.waitFor(() => t.b.eval(`document.querySelectorAll('.filmstrip a.thumb').length === 6`), 'six thumbnails');
    await t.b.key('j'); await waitAt(1);
    await t.b.key('k'); await waitAt(0);
    await t.b.key('G', [KEY.shift]); await waitAt(5);
    await t.b.key('g'); await t.b.key('g'); await waitAt(0);
    await clickIn(t, `.filmstrip a.thumb[href$="/images/${ids[2]}"]`); await waitAt(2);

    await t.b.key(KEY.enter, [KEY.ctrl]);
    await waitAt(3);
    const image = await t.api.call('GET', `/api/projects/${t.project.id}/images/${ids[2]}`);
    equal(image.annotated, true, 'marked annotated');
    await t.b.waitFor(() => t.b.eval(`document.querySelector('.filmstrip a.thumb[href$="/images/${ids[2]}"]')?.classList.contains('annotated')`), 'the filmstrip dot');
    await clickIn(t, 'button.mark-annotated');
    await t.b.waitFor(async () => (await t.api.call('GET', `/api/projects/${t.project.id}/images/${ids[3]}`)).annotated, 'the button marking the current image');
  },
});

// Creating a project (design A: a dialog, then a get-started checklist).
tests.push({
  name: 'New project opens a dialog that copies labels and imports a folder, then opens the project',
  async run(t) {
    await t.b.goto(`${t.base}/`);
    const card = `a.project-card[href="/projects/${t.project.id}"]`;
    await t.b.waitFor(() => t.b.eval(`!!document.querySelector(${JSON.stringify(card)})`), 'the project card');
    const text = await t.b.eval(`document.querySelector(${JSON.stringify(card)}).textContent`);
    if (!text.includes('1 image') || !text.includes('9 labels')) throw new Error(`card text: ${text}`);

    await t.b.eval(`document.querySelector('button.new-project').click(), 1`);
    await t.b.waitFor(() => t.b.eval(`!!document.querySelector('[role=dialog][aria-label="New project"]')`), 'the dialog');
    const box = await t.b.json(`document.querySelector('[role=dialog] input.project-name').getBoundingClientRect()`);
    await t.b.click(box.x + 20, box.y + box.height / 2);
    for (const ch of 'Copied') await t.b.key(ch);
    await t.b.eval(`(() => { const s = document.querySelector('[role=dialog] select.copy-from'); s.value = '${t.project.id}'; s.dispatchEvent(new Event('change', { bubbles: true })); return 1; })()`);
    // The host's folder dialog "picks" the image root.
    writeFileSync(join(t.work, 'folder-dialog.txt'), join(t.work, 'images'));
    await t.b.eval(`[...document.querySelectorAll('[role=dialog] button')].find((b) => b.textContent.startsWith('Browse')).click(), 1`);
    await t.b.waitFor(() => t.b.eval(`/\\d+ images? found/.test(document.querySelector('.folder-count')?.textContent ?? '')`), 'the image count');
    await t.b.eval(`document.querySelector('[role=dialog] button.primary').click(), 1`);

    const id = await t.b.waitFor(async () => {
      const match = /^\/projects\/(\d+)$/.exec(await t.b.eval('location.pathname'));
      return match && Number(match[1]) !== t.project.id && Number(match[1]);
    }, 'the new project to open');
    const source = await t.api.call('GET', `/api/projects/${t.project.id}/labels`);
    const copied = await t.api.call('GET', `/api/projects/${id}/labels`);
    equal(copied, source, 'copied labels');
    const page = await t.b.waitFor(async () => {
      const page = await t.api.call('GET', `/api/projects/${id}/images`);
      return page.total > 0 && page;
    }, 'the imported images');
    equal(page.images.some((image) => image.path === 'test.png'), true, 'the test image');
  },
}, {
  name: 'an empty project shows a get-started checklist whose import button opens the import dialog',
  async run(t) {
    const project = await t.api.call('POST', '/api/projects', { name: 'Empty' });
    await t.b.goto(`${t.base}/projects/${project.id}`);
    await t.b.waitFor(() => t.b.eval(`!!document.querySelector('.get-started')`), 'the checklist');
    const steps = await t.b.json(`[...document.querySelectorAll('.get-started li strong')].map((s) => s.textContent.trim())`);
    equal(steps.map((s) => s.split(' ')[0]), ['Labels', 'Images', 'Jobs'], 'steps');
    await t.b.eval(`[...document.querySelectorAll('.get-started button')].find((b) => b.textContent.startsWith('Import images')).click(), 1`);
    await t.b.waitFor(() => t.b.eval(`!!document.querySelector('.backdrop .dialog')`), 'the import dialog');
  },
});

tests.push({
  name: 'the New project dialog can create labels: Enter adds one with the next color and key',
  async run(t) {
    await t.b.goto(`${t.base}/`);
    await t.b.waitFor(() => t.b.eval(`!!document.querySelector('button.new-project')`), 'the projects page');
    await t.b.eval(`document.querySelector('button.new-project').click(), 1`);
    await t.b.waitFor(() => t.b.eval(`!!document.querySelector('[role=dialog] input.project-name')`), 'the dialog');
    const typeInto = async (selector, text) => {
      const box = await t.b.json(`document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect()`);
      await t.b.click(box.x + 20, box.y + box.height / 2);
      for (const ch of text) await t.b.key(ch);
    };
    await typeInto('[role=dialog] input.project-name', 'Fresh');
    await t.b.eval(`document.querySelector('[role=dialog] input[value=new]').click(), 1`);
    for (const name of ['crack', 'chip', 'spare']) {
      await typeInto('[role=dialog] input.new-label', name);
      await t.b.key(KEY.enter);
    }
    await t.b.eval(`[...document.querySelectorAll('[role=dialog] .new-labels li')].find((li) => li.textContent.includes('spare')).querySelector('button').click(), 1`);
    equal(await t.b.json(`[...document.querySelectorAll('[role=dialog] .new-labels li .name')].map((n) => n.textContent)`), ['crack', 'chip'], 'labels in the dialog');
    equal(await t.b.eval(`location.pathname`), '/', 'Enter added labels, it did not create the project');
    await t.b.eval(`document.querySelector('[role=dialog] button.primary').click(), 1`);
    const id = await t.b.waitFor(async () => {
      const match = /^\/projects\/(\d+)$/.exec(await t.b.eval('location.pathname'));
      return match && Number(match[1]);
    }, 'the new project to open');
    const labels = await t.api.call('GET', `/api/projects/${id}/labels`);
    equal(labels.map((l) => [l.name, l.hotkey]), [['crack', '1'], ['chip', '2']], 'created labels');
    if (labels[0].color === labels[1].color) throw new Error('the labels got the same color');
  },
});

// Images from any host folder, or uploaded (ADR 0003).
const answerFolderDialog = (t, answer) => writeFileSync(join(t.work, 'folder-dialog.txt'), answer);
const newProjectNamed = async (t, name) => {
  await t.b.goto(`${t.base}/`);
  await t.b.waitFor(() => t.b.eval(`!!document.querySelector('button.new-project')`), 'the projects page');
  await t.b.eval(`document.querySelector('button.new-project').click(), 1`);
  await t.b.waitFor(() => t.b.eval(`!!document.querySelector('[role=dialog] input.project-name')`), 'the dialog');
  const box = await t.b.json(`document.querySelector('[role=dialog] input.project-name').getBoundingClientRect()`);
  await t.b.click(box.x + 20, box.y + box.height / 2);
  for (const ch of name) await t.b.key(ch);
};
const browse = (t) => t.b.eval(`[...document.querySelectorAll('[role=dialog] button')].find((b) => b.textContent.startsWith('Browse')).click(), 1`);
const createAndOpen = async (t) => {
  await t.b.eval(`document.querySelector('[role=dialog] button.primary').click(), 1`);
  return t.b.waitFor(async () => {
    const match = /^\/projects\/(\d+)$/.exec(await t.b.eval('location.pathname'));
    return match && Number(match[1]) !== t.project.id && Number(match[1]);
  }, 'the new project to open', 20000);
};
const importedPaths = (t, id) => t.b.waitFor(async () => {
  const page = await t.api.call('GET', `/api/projects/${id}/images`);
  return page.total > 0 && page.images.map((image) => image.path);
}, 'the imported images', 20000);

tests.push({
  name: 'Browse… on the server\'s machine opens its folder dialog and imports the folder where it is',
  async run(t) {
    const folder = join(t.work, 'host-pictures');
    writeTestImage(join(folder, 'a.png'), 64, 48);
    writeTestImage(join(folder, 'deeper', 'b.png'), 48, 64);
    answerFolderDialog(t, folder);
    await newProjectNamed(t, 'Host folder');
    await browse(t);
    await t.b.waitFor(() => t.b.eval(`/^2 images found/.test(document.querySelector('.folder-count')?.textContent ?? '')`), 'the count');
    const id = await createAndOpen(t);
    const paths = await importedPaths(t, id);
    equal(paths.sort(), [join(folder, 'a.png'), join(folder, 'deeper', 'b.png')], 'referenced where they are');
  },
}, {
  name: 'when the server cannot show its dialog, Browse… picks the folder in the browser and uploads the images',
  async run(t) {
    const folder = join(t.work, 'browser-pictures');
    writeTestImage(join(folder, 'one.png'), 64, 48);
    writeTestImage(join(folder, 'two.png'), 48, 64);
    answerFolderDialog(t, 'unavailable');
    await newProjectNamed(t, 'Uploaded');
    await browse(t);
    await t.b.waitFor(() => t.b.eval(`!!document.querySelector('[role=dialog] input.pick-folder')`), 'the browser folder input');
    // WebDriver cannot hand a folder to the input, so it gets the folder's files (without their
    // folder path, which a real folder dialog adds).
    await t.b.setFiles('[role=dialog] input.pick-folder', [join(folder, 'one.png'), join(folder, 'two.png')]);
    await t.b.waitFor(() => t.b.eval(`/^2 images to upload/.test(document.querySelector('.folder-count')?.textContent ?? '')`), 'the count');
    const id = await createAndOpen(t).catch(async (err) => {
      throw new Error(`${err.message}; dialog says: ${await t.b.eval(`document.querySelector('[role=dialog] .error')?.textContent ?? 'nothing'`)}`);
    });
    const paths = await importedPaths(t, id);
    equal(paths.length, 2, 'two images');
    if (!paths.every((p) => p.startsWith(join(t.work, 'uploads')))) throw new Error(`not kept by the server: ${paths}`);
  },
});

// Export (ticket 06).
tests.push({
  name: 'the export dialog writes a YOLO dataset and offers it as a zip',
  async run(t) {
    await t.api.setAnnotations(t.project, [box(1, 100, 100, 300, 200)]);
    await t.b.goto(`${t.base}/projects/${t.project.id}`);
    await t.b.waitFor(() => t.b.eval(`!!document.querySelector('.grid li')`), 'the grid');
    const clickText = async (selector, text) => {
      const rect = await t.b.json(`[...document.querySelectorAll(${JSON.stringify(selector)})].find((e) => e.textContent.includes(${JSON.stringify(text)})).getBoundingClientRect()`);
      await t.b.click(rect.x + rect.width / 2, rect.y + rect.height / 2);
    };
    await clickText('.actions button', 'Export');
    await t.b.waitFor(() => t.b.eval(`!!document.querySelector('.export-dialog')`), 'the export dialog');
    await t.b.waitFor(() => t.b.eval(`!!document.querySelector('.export-dialog option')`), 'the formats');
    const offered = await t.b.json(`[...document.querySelectorAll('.export-dialog fieldset:last-of-type .choice')].filter((c) => getComputedStyle(c).display !== 'none').map((c) => [c.querySelector('span').firstChild.textContent, c.querySelector('input').checked])`);
    equal(offered, [['Links to the originals', true], ['Copies', false]], 'image options for YOLO');
    await clickText('.export-dialog .choice', 'Images with annotations');
    await clickText('.export-dialog .dialog-foot button', 'Export');
    const report = await t.b.waitFor(() => t.b.eval(`document.querySelector('.export-dialog .report')?.textContent`), 'the export result');
    if (!report.includes('Exported 1 images') || !report.includes('1 annotations')) throw new Error(`report: ${report}`);
    const href = await t.b.eval(`document.querySelector('.export-dialog a[download]').getAttribute('href')`);
    const zip = await t.api.bytes(href);
    equal(zip.subarray(0, 2).toString(), 'PK', 'zip signature');
    if (!zip.includes('images/train/') && !zip.includes('images/val/')) throw new Error('the zip has no images');
  },
}, {
  name: 'the export dialog offers YOLO segment and OBB, and a segment export includes polylines',
  async run(t) {
    // "line" (3) allows polylines; it is expanded with the label's line width.
    await t.api.setAnnotations(t.project, [{ label: 3, shape: { type: 'polyline', points: [{ x: 100, y: 100 }, { x: 300, y: 120 }] } }]);
    await t.b.goto(`${t.base}/projects/${t.project.id}`);
    await t.b.waitFor(() => t.b.eval(`!!document.querySelector('.grid li')`), 'the grid');
    await t.b.eval(`[...document.querySelectorAll('.actions button')].find((b) => b.textContent.includes('Export')).click(), 1`);
    await t.b.waitFor(() => t.b.eval(`!!document.querySelector('.export-dialog option')`), 'the formats');
    equal(await t.b.json(`[...document.querySelectorAll('.export-dialog option')].map((o) => o.textContent)`), ['YOLO detect', 'YOLO segment', 'YOLO OBB'], 'formats');
    await t.b.eval(`(() => { const s = document.querySelector('.export-dialog select'); s.value = 'yolo-segment'; s.dispatchEvent(new Event('change', { bubbles: true })); return 1; })()`);
    await t.b.eval(`[...document.querySelectorAll('.export-dialog .dialog-foot button')].find((b) => b.textContent.includes('Export')).click(), 1`);
    const report = await t.b.waitFor(() => t.b.eval(`document.querySelector('.export-dialog .report')?.textContent`), 'the export result');
    if (!report.includes('1 annotations')) throw new Error(`report: ${report}`);
  },
});

export const LABELS = NINE_LABELS;

// View filters (ticket 13).

/** The gray level the viewer canvas shows at screen point `at`. */
const viewerGray = (t, at) => t.b.json(`(() => {
  const canvas = document.querySelector('canvas.viewer');
  const rect = canvas.getBoundingClientRect();
  const ratio = canvas.width / rect.width;
  const [r] = canvas.getContext('2d').getImageData(Math.round((${at[0]} - rect.x) * ratio), Math.round((${at[1]} - rect.y) * ratio), 1, 1).data;
  return r;
})()`);
const openFilterPanel = (t) => t.b.eval(`document.querySelector('.view-filter.collapsed .collapse')?.click(), 1`);
const pressPreset = (t, name) => t.b.eval(`[...document.querySelectorAll('.view-filter .presets button')].find((b) => b.textContent === ${JSON.stringify(name)}).click(), 1`);
const tileRequests = (t, kind) => t.b.json(`performance.getEntriesByType('resource').map((e) => e.name).filter((n) => n.includes('/${kind}/'))`);

tests.push({
  name: 'a view filter preset and its sliders change the image, O shows the original, and the settings are remembered',
  async run(t) {
    const w = await openWorkspace(t.b, t.base, t.project);
    const spot = w.at(1250, 750);
    // The test image's smooth gradient there is about (1250 + 750) / 2600 × 200 ≈ 154.
    const original = await t.b.waitFor(async () => { const g = await viewerGray(t, spot); return g > 100 && g; }, 'the original image');
    near([original], [154], 6, 'original gray');
    equal(await t.b.eval(`document.querySelector('.view-chip').textContent`), 'View Original', 'header');
    equal(await t.b.eval(`!!document.querySelector('.view-filter.collapsed')`), true, 'the panel starts collapsed');

    await openFilterPanel(t);
    await pressPreset(t, 'LCN');
    // enhance_v2 flattens the lighting: the gradient becomes mid gray.
    const filtered = await t.b.waitFor(async () => { const g = await viewerGray(t, spot); return Math.abs(g - original) > 15 && g; }, 'the filtered image');
    equal(await t.b.eval(`document.querySelector('.view-chip').textContent`), 'View LCN', 'header with a filter');
    equal((await tileRequests(t, 'tiles')).some((n) => n.includes('?filter=lcn_0.030_6.0_0.020_0.50_3.0')), true, 'filtered tiles asked for');

    // Holding O shows the original, releasing it the filter again.
    await t.b.keyDown('o');
    await t.b.waitFor(async () => Math.abs((await viewerGray(t, spot)) - original) <= 2, 'the original while O is held');
    equal(await t.b.eval(`document.querySelector('.view-chip').textContent`), 'View Original', 'header while O is held');
    await t.b.keyUp('o');
    await t.b.waitFor(async () => Math.abs((await viewerGray(t, spot)) - filtered) <= 2, 'the filter again');

    // A slider asks for the filter with the new value once it rests.
    await t.b.eval(`(() => {
      const gain = [...document.querySelectorAll('.view-filter .slider')].find((s) => s.textContent.startsWith('Gain')).querySelector('input');
      gain.value = '1.5';
      gain.dispatchEvent(new Event('input', { bubbles: true }));
      return 1;
    })()`);
    await t.b.waitFor(async () => (await tileRequests(t, 'tiles')).some((n) => n.includes('?filter=lcn_0.030_6.0_0.020_1.50_3.0')), 'tiles of the changed gain');

    // Brightness and contrast are applied on screen.
    await t.b.eval(`(() => {
      const brightness = [...document.querySelectorAll('.view-filter .slider')].find((s) => s.textContent.startsWith('Brightness')).querySelector('input');
      brightness.value = '150';
      brightness.dispatchEvent(new Event('input', { bubbles: true }));
      return 1;
    })()`);
    equal(await t.b.eval(`document.querySelector('canvas.viewer').style.filter`), 'brightness(150%) contrast(100%)', 'CSS filter');

    // Drawing on the filtered view stores original image coordinates.
    await t.b.key('r');
    await t.b.drag(w.at(900, 500), w.at(1300, 800));
    const [annotation] = await saved(t, (list) => list.length === 1, 'the rectangle');
    near(corners(annotation), [900, 500, 1300, 800], 2 / w.scale, 'rectangle on the filtered view');

    // Remembered for the project in this browser.
    await t.b.goto(`${t.base}/projects/${t.project.id}/images/${t.project.image}`);
    await t.b.waitFor(() => t.b.eval(`document.querySelector('.view-chip')?.textContent === 'View LCN'`), 'the filter after reloading');
    equal(await t.b.json(`[...document.querySelectorAll('.view-filter .slider .value')].map((v) => v.textContent)`),
      ['0.030', '6.0', '0.020', '1.50', '3.0', '150%', '100%'], 'remembered values');
  },
}, {
  name: 'the fuzzy fill can analyse the filtered view',
  async run(t) {
    const w = await openWorkspace(t.b, t.base, t.project);
    await t.b.key('u');
    const filtered = () => t.b.json(`[...document.querySelectorAll('.tool-options button')].find((b) => b.textContent === 'Filtered view').disabled`);
    equal(await filtered(), true, 'no filter: the switch is off');
    await openFilterPanel(t);
    await pressPreset(t, 'CLAHE');
    await t.b.waitFor(async () => !(await filtered()), 'the switch with a filter');
    await clickOption(t, 'Filtered view');
    await t.b.click(...w.at(1150, 650));
    await t.b.waitFor(async () => (await tileRequests(t, 'analysis')).some((n) => n.includes('?filter=clahe_0.05_3.0_16')), 'filtered analysis tiles');
    await saved(t, (list) => list.length === 1 && list[0].shape.type === 'mask', 'the fill');
  },
});

tests.push({
  name: 'an admin adds an annotator and resets their password; the annotator sees no admin actions and logs out',
  async run(t) {
    const anna = { username: 'anna', password: 'first password' };
    const clickButton = (selector, text) => t.b.eval(`[...document.querySelectorAll(${JSON.stringify(selector)})].find((b) => b.textContent.trim() === ${JSON.stringify(text)}).click(), 1`);
    const fill = (selector, value) => t.b.eval(`(() => {
      const input = document.querySelector(${JSON.stringify(selector)});
      input.value = ${JSON.stringify(value)};
      input.dispatchEvent(new Event('input', { bubbles: true }));
      return 1;
    })()`);
    const row = (name) => `.users tr[data-user="${name}"]`;
    const logOut = async () => {
      await t.b.eval(`document.querySelector('.account button.log-out').click(), 1`);
      await t.b.waitFor(() => t.b.eval(`!!document.querySelector('form[aria-label="Sign in"]')`), 'the sign-in form');
    };

    await t.b.goto(`${t.base}/`);
    await t.b.waitFor(() => t.b.eval(`!!document.querySelector('.account a[href="/users"]')`), 'the Users link');
    await t.b.eval(`document.querySelector('.account a[href="/users"]').click(), 1`);
    await t.b.waitFor(() => t.b.eval(`!!document.querySelector(${JSON.stringify(row('admin'))})`), 'the users table');
    await fill('.new-user input.username', anna.username);
    await fill('.new-user input.password', anna.password);
    await clickButton('.new-user button', 'Add user');
    await t.b.waitFor(() => t.b.eval(`document.querySelector(${JSON.stringify(row('anna') + ' select.role')})?.value === 'annotator'`), 'anna as annotator');

    await clickButton(`${row('anna')} button`, 'Reset password…');
    await fill(`${row('anna')} input.new-password`, 'second password');
    await clickButton(`${row('anna')} button`, 'Set');
    await t.b.waitFor(() => t.b.eval(`document.querySelector('[role=status]')?.textContent.includes("anna's password was reset")`), 'the reset notice');
    anna.password = 'second password';
    await api(t.base, anna);
    // Annotators see the projects with jobs of theirs.
    const annaId = (await t.api.call('GET', '/api/users')).find((user) => user.username === 'anna').id;
    await t.api.call('POST', `/api/projects/${t.project.id}/jobs`, { image_ids: [t.project.image], assignee: annaId });

    await logOut();
    try {
      await signIn(t.b, t.base, anna);
      await t.b.waitFor(() => t.b.eval(`!!document.querySelector('a.project-card')`), 'the project cards');
      const hidden = await t.b.json(`({
        users: !!document.querySelector('.account a[href="/users"]'),
        newProject: !!document.querySelector('button.new-project, button.new-card'),
        deleteProject: !!document.querySelector('.project-card .delete'),
      })`);
      equal(hidden, { users: false, newProject: false, deleteProject: false }, 'admin controls on the projects page');
      await t.b.goto(`${t.base}/projects/${t.project.id}/labels`);
      await t.b.waitFor(() => t.b.eval(`!!document.querySelector('table.labels')`), 'the labels table');
      const projectHidden = await t.b.json(`({
        importExport: [...document.querySelectorAll('.topbar button')].some((b) => /Import|Export/.test(b.textContent)),
        labelForm: !!document.querySelector('form.label-form'),
        labelButtons: document.querySelectorAll('table.labels td.actions button').length,
      })`);
      equal(projectHidden, { importExport: false, labelForm: false, labelButtons: 0 }, 'admin controls on the project page');

      // A password reset ends anna's session: her next request brings back the sign-in form.
      await t.api.call('PUT', `/api/users/${annaId}`, { password: 'third password' });
      await t.b.eval(`document.querySelector('a.tab[href="/projects/${t.project.id}"]').click(), 1`);
      await t.b.waitFor(() => t.b.eval(`!!document.querySelector('form[aria-label="Sign in"]')`), 'the sign-in form after the reset');

      await signIn(t.b, t.base, { username: 'anna', password: 'third password' });
      await t.b.goto(`${t.base}/users`);
      await t.b.waitFor(() => t.b.eval(`document.querySelector('main.users')?.textContent.includes('Only admins manage users.')`), 'the Users page refusing anna');
      equal(await t.b.eval(`!!document.querySelector('form.new-user')`), false, 'the add-user form for anna');
      await logOut();
    } finally {
      await signOut(t.b, t.base);
      await signIn(t.b, t.base, ADMIN);
    }
  },
});

tests.push({
  name: 'an admin splits the selection into assigned jobs; the annotator sees and annotates only their job',
  async run(t) {
    const p = t.project.id;
    // Different sizes, as an import skips images with the same content.
    writeTestImage(join(t.work, 'images', 'jobs/b.png'), 200, 100);
    writeTestImage(join(t.work, 'images', 'jobs/c.png'), 220, 100);
    await t.api.call('POST', `/api/projects/${p}/imports`, { paths: ['jobs'] });
    const anna = { username: 'jobs-anna', password: 'anna password' };
    const user = await t.api.call('POST', '/api/users', { ...anna, role: 'annotator' });
    const gridCount = () => t.b.eval(`document.querySelectorAll('.grid li').length`);
    const select = (selector, value) => t.b.eval(`(() => {
      const s = document.querySelector(${JSON.stringify(selector)});
      s.value = ${JSON.stringify(String(value))};
      s.dispatchEvent(new Event('change', { bubbles: true }));
      return 1;
    })()`);

    await t.b.goto(`${t.base}/projects/${p}`);
    await t.b.waitFor(async () => (await gridCount()) === 3, 'three images in the grid');
    await t.b.eval(`[...document.querySelectorAll('.toolbar button')].find((b) => b.textContent === 'Select all').click(), 1`);
    await t.b.waitFor(() => t.b.eval(`!!document.querySelector('button.create-jobs')`), 'the Create jobs button');
    await t.b.eval(`document.querySelector('button.create-jobs').click(), 1`);
    await t.b.waitFor(() => t.b.eval(`!!document.querySelector('.jobs-dialog')`), 'the dialog');
    await t.b.eval(`document.querySelector('.jobs-dialog input.split').click(), 1`);
    await t.b.eval(`(() => { const i = document.querySelector('.jobs-dialog input.per-job'); i.value = '2'; i.dispatchEvent(new Event('input', { bubbles: true })); return 1; })()`);
    await t.b.waitFor(() => t.b.eval(`document.querySelectorAll('.jobs-dialog select.assignee option').length > 1`), 'the users');
    await select('.jobs-dialog select.assignee', user.id);
    equal(await t.b.eval(`document.querySelector('.jobs-dialog button.primary').textContent`), 'Create 2 jobs', 'button');
    await t.b.eval(`document.querySelector('.jobs-dialog button.primary').click(), 1`);

    await t.b.waitFor(() => t.b.eval(`location.pathname === '/projects/${p}/jobs' && document.querySelectorAll('.jobs-table tbody tr').length === 2`), 'the Jobs tab with two jobs');
    const jobs = await t.api.call('GET', `/api/projects/${p}/jobs`);
    equal(jobs.map((j) => [j.number, j.images, j.assignee?.username]), [[1, 2, anna.username], [2, 1, anna.username]], 'jobs');
    // Job 2 goes back to nobody.
    await select('tr[data-job="2"] select.assignee', '');
    await t.b.waitFor(async () => (await t.api.call('GET', `/api/projects/${p}/jobs`))[1].assignee === null, 'job 2 unassigned');

    await t.b.eval(`document.querySelector('a.tab[href="/projects/${p}/statistics"]').click(), 1`);
    await t.b.waitFor(() => t.b.eval(`document.querySelectorAll('.per-job tbody tr').length === 2 && document.querySelectorAll('.per-user tbody tr').length === 2`), 'the statistics');

    await t.b.eval(`document.querySelector('a.tab[href="/projects/${p}"]').click(), 1`);
    await t.b.waitFor(() => t.b.eval(`document.querySelectorAll('select.job-filter option').length === 4`), 'the job menu');
    await select('select.job-filter', jobs[0].id);
    await t.b.waitFor(async () => (await gridCount()) === 2, 'the grid showing job 1');

    await signOut(t.b, t.base);
    try {
      await signIn(t.b, t.base, anna);
      await t.b.waitFor(() => t.b.eval(`document.querySelectorAll('a.project-card').length === 1`), 'only the project with her job');
      await t.b.goto(`${t.base}/projects/${p}`);
      await t.b.waitFor(async () => (await gridCount()) === 2, 'only her two images');
      await t.b.goto(`${t.base}/projects/${p}/jobs`);
      await t.b.waitFor(() => t.b.eval(`document.querySelectorAll('.jobs-table tbody tr').length === 1`), 'only her job');
      equal(await t.b.eval(`!!document.querySelector('a.tab[href$="/statistics"]')`), false, 'statistics tab for annotators');
      await t.b.eval(`document.querySelector('.jobs-table button.annotate').click(), 1`);
      await t.b.waitFor(() => t.b.eval(`/^\\/projects\\/${p}\\/images\\/\\d+$/.test(location.pathname) && !!document.querySelector('canvas.annotations')`), 'the workspace');
    } finally {
      await signOut(t.b, t.base);
      await signIn(t.b, t.base, ADMIN);
    }
  },
});

tests.push({
  name: 'a CVAT export is previewed, its labels mapped, created or left out, and then imported',
  async run(t) {
    const p = t.project.id;
    const xml = `<?xml version="1.0" encoding="utf-8"?>
<annotations><version>1.1</version>
  <meta><project><labels>
    <label><name>crack</name><color>#e6194b</color><type>any</type><attributes></attributes></label>
    <label><name>newbie</name><color>#123456</color><type>any</type><attributes></attributes></label>
  </labels></project></meta>
  <image id="0" name="cvat/test.png" width="1600" height="1000">
    <box label="crack" source="manual" occluded="0" xtl="100" ytl="100" xbr="200" ybr="150" z_order="0"></box>
    <ellipse label="newbie" source="manual" occluded="0" cx="400" cy="400" rx="50" ry="20" rotation="30" z_order="1"></ellipse>
  </image>
  <image id="1" name="missing.png" width="10" height="10"></image>
</annotations>`;
    const file = join(t.work, 'cvat-export.xml');
    writeFileSync(file, xml);
    const importButton = () => t.b.eval(`document.querySelector('.cvat-dialog button.import')?.textContent ?? ''`);
    const choose = (label, value) => t.b.eval(`(() => {
      const s = document.querySelector('.cvat-labels tr[data-label="${label}"] select');
      s.value = ${JSON.stringify(value)};
      s.dispatchEvent(new Event('change', { bubbles: true }));
      return 1;
    })()`);

    await t.b.goto(`${t.base}/projects/${p}`);
    await t.b.waitFor(() => t.b.eval(`!!document.querySelector('button.import-annotations')`), 'the button');
    await t.b.eval(`document.querySelector('button.import-annotations').click(), 1`);
    await t.b.waitFor(() => t.b.eval(`!!document.querySelector('.cvat-dialog input.cvat-file')`), 'the dialog');
    await t.b.setFiles('.cvat-dialog input.cvat-file', [file]);
    await t.b.waitFor(() => t.b.eval(`document.querySelector('.cvat-summary')?.textContent === '1 of 2 images match.'`), 'the preview');
    const choices = await t.b.json(`[...document.querySelectorAll('.cvat-labels tbody tr')].map((r) => [r.dataset.label, r.querySelector('select').selectedOptions[0].textContent])`);
    equal(choices, [['crack', '→ crack'], ['newbie', 'New label']], 'suggested choices');
    equal(await t.b.eval(`document.querySelector('.cvat-unmatched summary').textContent`), '1 image with no project image of that file name', 'unmatched');
    equal(await importButton(), 'Import 2 annotations', 'button before');

    await choose('newbie', 'skip');
    await t.b.waitFor(async () => (await importButton()) === 'Import 1 annotation', 'the preview without newbie');
    equal(await t.b.eval(`document.querySelector('.cvat-skipped li').textContent`), '1 × label "newbie" is skipped', 'skipped');
    await choose('newbie', 'create');
    await t.b.waitFor(async () => (await importButton()) === 'Import 2 annotations', 'the preview with newbie');
    // The export's other image is not in the project, but in a folder of the image root.
    writeTestImage(join(t.work, 'images', 'cvat-extra', 'sub', 'missing.png'), 10, 10);
    await t.b.eval(`(() => {
      const input = document.querySelector('.cvat-dialog input.images-folder-path');
      input.value = 'cvat-extra';
      input.dispatchEvent(new Event('change', { bubbles: true }));
      return 1;
    })()`);
    await t.b.waitFor(() => t.b.eval(`document.querySelector('.cvat-summary')?.textContent === '2 of 2 images match, 1 of them found in the images folder and to be added.'`), 'the folder preview');
    await t.b.eval(`document.querySelector('.cvat-dialog button.import').click(), 1`);
    await t.b.waitFor(() => t.b.eval(`document.querySelector('.cvat-result')?.textContent === 'Imported 2 annotations into 2 images, adding 1 image to the project.'`), 'the result');
    const page = await t.api.call('GET', `/api/projects/${p}/images`);
    equal(page.images.map((image) => image.path), ['cvat-extra/sub/missing.png', 'test.png'], 'project images');

    const { annotations } = await t.api.annotations(t.project);
    equal(annotations.map((a) => a.shape.type), ['rectangle', 'ellipse'], 'imported shapes');
    const labels = await t.api.call('GET', `/api/projects/${p}/labels`);
    equal(labels.at(-1).name, 'newbie', 'created label');
    equal(annotations[1].label, labels.at(-1).id, 'ellipse label');
  },
});

tests.push({
  name: 'a model registered on the Models screen pre-annotates an image; its results are reviewed by confidence',
  async run(t) {
    const p = t.project.id;
    const adapter = join(dirname(fileURLToPath(import.meta.url)), 'test_adapter.py');
    const fill = (selector, value) => t.b.eval(`(() => {
      const input = document.querySelector(${JSON.stringify(selector)});
      input.value = ${JSON.stringify(value)};
      input.dispatchEvent(new Event('input', { bubbles: true }));
      return 1;
    })()`);

    await t.b.goto(`${t.base}/`);
    await t.b.waitFor(() => t.b.eval(`!!document.querySelector('.account a[href="/models"]')`), 'the Models link');
    await t.b.eval(`document.querySelector('.account a[href="/models"]').click(), 1`);
    await t.b.waitFor(() => t.b.eval(`document.querySelector('.worker-health')?.textContent.includes('Worker ready on')`), 'the worker', 30000);
    await fill('.model-form input.model-name', 'ui model');
    await fill('.model-form input.model-adapter', adapter);
    await fill('.model-form input.model-weights', 'none.pt');
    await t.b.eval(`document.querySelector('.model-form button.primary').click(), 1`);
    await t.b.waitFor(() => t.b.eval(`!!document.querySelector('.models-table tr[data-model="ui model"]')`), 'the model row');
    await t.b.eval(`[...document.querySelectorAll('.models-table tr[data-model="ui model"] button')].find((b) => b.textContent === 'Edit').click(), 1`);
    await t.b.waitFor(() => t.b.eval(`!!document.querySelector('.model-form button.read-classes')`), 'the edit form');
    await t.b.eval(`document.querySelector('.model-form button.read-classes').click(), 1`);
    await t.b.waitFor(() => t.b.eval(`document.querySelector('.model-form textarea.model-classes').value === 'crack = crack\\nfinger = finger\\n'`), 'the classes');
    await t.b.eval(`document.querySelector('.model-form button.primary').click(), 1`);
    await t.b.waitFor(async () => (await t.api.call('GET', '/api/models'))[0]?.class_map?.finger === 'finger', 'the saved class map');

    const w = await openWorkspace(t.b, t.base, t.project);
    await t.b.waitFor(() => t.b.eval(`!!document.querySelector('.pre-annotate button.run:not([disabled])')`), 'the Run button');
    await t.b.eval(`document.querySelector('.pre-annotate button.run').click(), 1`);
    await t.b.waitFor(() => t.b.eval(`document.querySelector('.pre-annotate .run-summary')?.textContent === 'Done: 2 pre-annotations.'`), 'the run');
    await t.b.waitFor(async () => (await w.state()).count === 2, 'the pre-annotations in the editor');
    equal(await t.b.json(`[...document.querySelectorAll('.objects li.pending')].length`), 2, 'pending rows');

    // From 0.5 up, only the crack shows; the finger is deleted, the crack accepted.
    await t.b.eval(`(() => { const r = document.querySelector('.review input[type=range]'); r.value = '0.5'; r.dispatchEvent(new Event('input', { bubbles: true })); return 1; })()`);
    await t.b.waitFor(() => t.b.eval(`document.querySelector('.review .delete-below').textContent === 'Delete 1 below'`), 'the threshold');
    await t.b.eval(`document.querySelector('.review .delete-below').click(), 1`);
    await t.b.eval(`document.querySelector('.review .accept-shown').click(), 1`);
    const [crack] = await saved(t, (list) => list.length === 1 && !list[0].pending, 'the reviewed crack');
    equal([crack.label, crack.source, crack.confidence], [1, 'ui model', 0.9], 'the accepted crack');
    equal(await t.b.eval(`!!document.querySelector('.review')`), false, 'nothing left to review');

    // The project's dialog runs it on the grid's selection.
    await t.b.goto(`${t.base}/projects/${p}`);
    await t.b.waitFor(() => t.b.eval(`!!document.querySelector('.grid li')`), 'the grid');
    await t.b.eval(`[...document.querySelectorAll('.toolbar button')].find((b) => b.textContent === 'Select all').click(), 1`);
    await t.b.waitFor(() => t.b.eval(`!!document.querySelector('.selection-bar')`), 'the selection');
    await t.b.eval(`document.querySelector('.topbar button.pre-annotate').click(), 1`);
    await t.b.waitFor(() => t.b.eval(`!!document.querySelector('.run-dialog button.run:not([disabled])')`), 'the dialog');
    equal(await t.b.eval(`document.querySelector('.run-dialog input.scope-selection').checked`), true, 'the selection is the scope');
    await t.b.eval(`document.querySelector('.run-dialog button.run').click(), 1`);
    await t.b.waitFor(() => t.b.eval(`document.querySelector('.run-dialog .run-summary')?.textContent === 'Done: 2 pre-annotations.'`), 'the dialog run');
    const { annotations } = await t.api.annotations(t.project);
    equal(annotations.map((a) => [a.label, !!a.pending]), [[1, false], [1, true], [2, true]], 'the reviewed crack stays; new pending ones are added');
  },
});

tests.push({
  name: 'a rectangle drawn while a model runs on the image is kept next to its pre-annotations',
  async run(t) {
    const adapter = join(dirname(fileURLToPath(import.meta.url)), 'test_adapter.py');
    await t.api.call('POST', '/api/models', { name: 'slow model', adapter, weights: 'none.pt', options: { delay: 2 } });
    const w = await openWorkspace(t.b, t.base, t.project);
    await t.b.waitFor(() => t.b.eval(`!!document.querySelector('.pre-annotate button.run:not([disabled])')`), 'the Run button');
    await t.b.eval(`(() => { const s = document.querySelector('.pre-annotate select.run-model'); s.value = [...s.options].find((o) => o.text === 'slow model').value; s.dispatchEvent(new Event('change', { bubbles: true })); return 1; })()`);
    await t.b.eval(`document.querySelector('.pre-annotate button.run').click(), 1`);
    await t.b.waitFor(() => t.b.eval(`!!document.querySelector('.pre-annotate button.cancel-run')`), 'the running run');
    await t.b.key('r');
    await t.b.drag(w.at(1000, 600), w.at(1200, 800));
    await t.b.waitFor(() => t.b.eval(`document.querySelector('.pre-annotate .run-summary')?.textContent === 'Done: 2 pre-annotations.'`), 'the run', 15000);
    const list = await saved(t, (list) => list.length === 3, 'the drawn rectangle and both pre-annotations');
    equal(list.map((a) => [a.source ?? 'manual', !!a.pending]), [['manual', false], ['slow model', true], ['slow model', true]], 'annotations');
    equal((await w.state()).save, 'Saved', 'save state');
  },
});

tests.push({
  name: 'the Settings screen shows where the worker is and saves a change',
  async run(t) {
    const before = await t.api.call('GET', '/api/settings/worker');
    try {
      await t.b.goto(`${t.base}/`);
      await t.b.waitFor(() => t.b.eval(`!!document.querySelector('.account a[href="/settings"]')`), 'the Settings link');
      await t.b.eval(`document.querySelector('.account a[href="/settings"]').click(), 1`);
      await t.b.waitFor(() => t.b.eval(`document.querySelector('.worker-settings input.worker-url')?.value === ${JSON.stringify(before.url)}`), 'the stored URL');
      equal(await t.b.eval(`document.querySelector('.worker-settings input.worker-command').value`), before.command, 'the stored command');
      await t.b.waitFor(() => t.b.eval(`document.querySelector('.worker-health')?.textContent.includes('Worker ready on')`), 'the worker', 30000);
      await t.b.eval(`(() => {
        const set = (selector, value) => { const i = document.querySelector(selector); i.value = value; i.dispatchEvent(new Event('input', { bubbles: true })); };
        set('.worker-settings input.worker-command', '');
        set('.worker-settings input.worker-url', 'http://127.0.0.1:9/');
        document.querySelector('.worker-settings button.primary').click();
        return 1;
      })()`);
      await t.b.waitFor(() => t.b.eval(`document.querySelector('.worker-settings .saved')?.textContent === 'Saved and applied.'`), 'the save');
      equal(await t.api.call('GET', '/api/settings/worker'), { command: null, url: 'http://127.0.0.1:9' }, 'stored settings');
      await t.b.waitFor(() => t.b.eval(`document.querySelector('.worker-health')?.textContent.includes('Worker not reachable')`), 'the unreachable worker', 10000);
    } finally {
      await t.api.call('PUT', '/api/settings/worker', before);
    }
  },
});

tests.push({
  name: 'a project archive with its images is downloaded and imported as a new project',
  async run(t) {
    const p = t.project.id;
    await t.api.setAnnotations(t.project, [box(1, 100, 100, 300, 200)]);
    await t.b.goto(`${t.base}/projects/${p}`);
    await t.b.waitFor(() => t.b.eval(`!!document.querySelector('.topbar button.archive')`), 'the Archive button');
    await t.b.eval(`document.querySelector('.topbar button.archive').click(), 1`);
    await t.b.waitFor(() => t.b.eval(`!!document.querySelector('.archive-dialog')`), 'the dialog');
    await t.b.eval(`document.querySelector('.archive-dialog input.with-images').click(), 1`);
    const href = await t.b.eval(`document.querySelector('.archive-dialog a.download-archive').getAttribute('href')`);
    equal(href, `/api/projects/${p}/archive?images=true`, 'download link');
    const file = join(t.work, 'project.rtag.zip');
    writeFileSync(file, await t.api.bytes(href));

    await t.b.goto(`${t.base}/`);
    await t.b.waitFor(() => t.b.eval(`!!document.querySelector('button.import-archive')`), 'the Import archive button');
    await t.b.eval(`document.querySelector('button.import-archive').click(), 1`);
    await t.b.waitFor(() => t.b.eval(`!!document.querySelector('.import-archive-dialog input.archive-file')`), 'the import dialog');
    await t.b.setFiles('.import-archive-dialog input.archive-file', [file]);
    await t.b.eval(`(() => { const i = document.querySelector('.import-archive-dialog input.archive-name'); i.value = 'Restored'; i.dispatchEvent(new Event('input', { bubbles: true })); return 1; })()`);
    await t.b.waitFor(() => t.b.eval(`!document.querySelector('.import-archive-dialog button.import-archive').disabled`), 'the file');
    await t.b.eval(`document.querySelector('.import-archive-dialog button.import-archive').click(), 1`);
    const id = await t.b.waitFor(async () => {
      const match = /^\/projects\/(\d+)$/.exec(await t.b.eval('location.pathname'));
      return match && Number(match[1]) !== p && Number(match[1]);
    }, 'the new project', 15000);
    const project = await t.api.call('GET', `/api/projects/${id}`);
    equal(project.name, 'Restored', 'name');
    const labels = await t.api.call('GET', `/api/projects/${id}/labels`);
    equal(labels, await t.api.call('GET', `/api/projects/${p}/labels`), 'labels with their ids');
    const page = await t.api.call('GET', `/api/projects/${id}/images`);
    const { annotations } = await t.api.call('GET', `/api/projects/${id}/images/${page.images[0].id}/annotations`);
    equal(annotations, (await t.api.annotations(t.project)).annotations, 'annotations');
  },
});

tests.push({
  name: 'own settings: a rebound key, flagged clashes, the flick distance, the label slice and the theme',
  async run(t) {
    const change = (action, how = 'change') => t.b.eval(`document.querySelector('.shortcuts-table tr[data-action="${action}"] button.${how}').click(), 1`);
    const stored = () => t.api.call('GET', '/api/me/settings');
    try {
      await t.b.goto(`${t.base}/settings`);
      await t.b.waitFor(() => t.b.eval(`!!document.querySelector('.shortcuts-table tr[data-action="rectangle"]')`), 'the shortcuts');
      await change('rectangle');
      await t.b.waitFor(() => t.b.eval(`!!document.querySelector('.shortcuts-table .capturing')`), 'waiting for a key');
      await t.b.key('w');
      await t.b.waitFor(async () => (await stored()).shortcuts.rectangle?.[0]?.key === 'w', 'the saved key');
      // W for polygon too: both rows are flagged.
      await change('polygon', 'add');
      await t.b.key('w');
      await t.b.waitFor(() => t.b.eval(`document.querySelector('.conflict-count')?.textContent === '1 key is used twice'`), 'the clash');
      equal(await t.b.json(`[...document.querySelectorAll('.shortcuts-table tr.conflict')].map((r) => r.dataset.action)`), ['rectangle', 'polygon'], 'flagged rows');
      await change('polygon', 'reset');
      await t.b.waitFor(() => t.b.eval(`!document.querySelector('.conflict-count')`), 'no clash');

      // Light theme at once, and the label slice; a far flick distance.
      await t.b.eval(`document.querySelector('.theme input[value="Light"]').click(), 1`);
      await t.b.waitFor(() => t.b.eval(`document.documentElement.dataset.theme === 'light'`), 'the light theme');
      await t.b.eval(`document.querySelector('.label-style input[value="options_ring"]').click(), 1`);
      await t.b.eval(`(() => { const r = document.querySelector('.flick-distance'); r.value = '400'; r.dispatchEvent(new Event('change', { bubbles: true })); return 1; })()`);
      await t.b.waitFor(async () => { const s = await stored(); return s.theme === 'light' && s.label_style === 'options_ring' && s.flick_distance === 400; }, 'the saved settings');

      // They follow into the workspace after a reload.
      const w = await openWorkspace(t.b, t.base, t.project);
      equal(await t.b.eval(`document.documentElement.dataset.theme`), 'light', 'theme after reload');
      await t.b.key('w');
      equal((await w.state()).tool, 'Rectangle', 'W picks the rectangle');
      await t.b.key('r');
      equal((await w.state()).tool, 'Rectangle', 'R does nothing now');
      // Held, a 170 px move picks by default (150 px); at 400 px the pie stays open.
      const [x, y] = w.at(800, 500);
      await t.b.move(x, y);
      await t.b.keyDown('q');
      await t.b.move(x - 120, y + 120, 120);
      await t.b.waitFor(async () => (await w.state()).pie === 'Rectangle options', 'the options pie still open');
      equal((await w.state()).pieItems.map((item) => item.name).includes('Label…'), true, 'the Label slice');
      // Released over the Label slice (south-west), it opens the label pie.
      await t.b.keyUp('q');
      await t.b.waitFor(async () => (await w.state()).pie === 'Labels', 'the label pie');
      await t.b.key(KEY.escape);
    } finally {
      await t.api.call('PUT', '/api/me/settings', {});
    }
  },
});

// Shape constraints and export footprints (ticket 24).
/** Changes label `id` of the test project through the API. */
const changeLabel = async (t, id, change) => {
  const labels = await t.api.call('GET', `/api/projects/${t.project.id}/labels`);
  const label = labels.find((l) => l.id === id);
  await t.api.call('PUT', `/api/projects/${t.project.id}/labels/${id}`, { ...label, ...change });
};
/** The opacity of the annotation canvas at image point (x, y). */
const overlayAlpha = (t, w, x, y) => {
  const [sx, sy] = w.at(x, y);
  return t.b.json(`(() => {
    const canvas = document.querySelector('canvas.annotations');
    const rect = canvas.getBoundingClientRect();
    const ratio = canvas.width / rect.width;
    return canvas.getContext('2d').getImageData(Math.round((${sx} - rect.x) * ratio), Math.round((${sy} - rect.y) * ratio), 1, 1).data[3];
  })()`);
};

tests.push({
  name: 'a fixed rectangle size set on the Labels tab places rectangles with one click, without resize handles',
  async run(t) {
    await t.b.goto(`${t.base}/projects/${t.project.id}/labels`);
    await t.b.waitFor(() => t.b.eval(`!!document.querySelector('table.labels')`), 'the labels');
    await t.b.eval(`document.querySelector('table.labels tbody tr button').click(), 1`);
    await t.b.waitFor(() => t.b.eval(`!!document.querySelector('.label-form .constraint input.fixed')`), 'the constraints');
    await t.b.eval(`document.querySelector('.label-form .constraint input.fixed').click(), 1`);
    const set = async (selector, value) => {
      await t.b.waitFor(() => t.b.eval(`!!document.querySelector(${JSON.stringify(selector)})`), selector);
      await t.b.eval(`(() => { const el = document.querySelector(${JSON.stringify(selector)}); el.value = ${JSON.stringify(value)}; el.dispatchEvent(new Event('input', { bubbles: true })); return 1; })()`);
    };
    await set('.label-form input.fixed-rectangle-w', '120');
    await set('.label-form input.fixed-rectangle-h', '80');
    await t.b.eval(`document.querySelector('.label-form .point-square').click(), 1`);
    await t.b.eval(`document.querySelector('.label-form button.primary').click(), 1`);
    const label = await t.b.waitFor(async () => {
      const crack = (await t.api.call('GET', `/api/projects/${t.project.id}/labels`)).find((l) => l.id === 1);
      return crack.constraints.rectangle && crack;
    }, 'the saved constraint');
    equal([label.constraints, label.point_shape], [{ rectangle: { width: 120, height: 80 }, oriented_rectangle: null, ellipse: null }, 'square'], 'constraint and point shape');
    equal(await t.b.eval(`document.querySelector('table.labels tbody tr .fixed').textContent`), 'Rectangle 120×80', 'the table');

    const w = await openWorkspace(t.b, t.base, t.project);
    await t.b.key('r');
    await t.b.click(...w.at(800, 500));
    let [placed] = await saved(t, (list) => list.length === 1, 'the placed rectangle');
    near(corners(placed), [740, 460, 860, 540], 2 / w.scale, 'a 120 × 80 rectangle centred on the click');
    near([placed.shape.max.x - placed.shape.min.x, placed.shape.max.y - placed.shape.min.y], [120, 80], 1e-6, 'its size');
    // Near the edge it moves inside the image.
    await t.b.click(...w.at(10, 990));
    [, placed] = await saved(t, (list) => list.length === 2, 'the second rectangle');
    near(corners(placed), [0, 920, 120, 1000], 0.01, 'kept inside');
    // Selected, its corner moves it instead of resizing it.
    await t.b.key('v');
    await t.b.click(...w.at(800, 500));
    await t.b.drag(w.at(860, 540), w.at(960, 600));
    [placed] = await saved(t, (list) => corners(list[0])[0] > 800, 'the move');
    near(corners(placed), [840, 520, 960, 600], 2 / w.scale, 'moved, same size');
  },
}, {
  name: 'a fixed oriented rectangle is placed by a click and turned to the pointer by a second click',
  async run(t) {
    await changeLabel(t, 1, { constraints: { oriented_rectangle: { width: 200, height: 100 } } });
    const w = await openWorkspace(t.b, t.base, t.project);
    await t.b.key('r');
    await t.b.eval(`[...document.querySelectorAll('.tool-options button')].find((b) => b.textContent.includes('3-point oriented')).click(), 1`);
    await t.b.click(...w.at(800, 500));
    await t.b.move(...w.at(800, 800), 60);
    await t.b.click(...w.at(800, 800));
    const [placed] = await saved(t, (list) => list.length === 1, 'the oriented rectangle');
    const { centre, width, height, angle } = placed.shape;
    near([centre.x, centre.y], [800, 500], 2 / w.scale, 'centre');
    near([width, height, angle], [200, 100, Math.PI / 2], 0.01, 'size and angle');
  },
}, {
  name: 'the footprint of a polyline shows as a ghost, toggled from the workspace header',
  async run(t) {
    try {
      await changeLabel(t, 3, { line_width: 60 });
      await t.api.setAnnotations(t.project, [{ label: 3, shape: { type: 'polyline', points: [{ x: 400, y: 500 }, { x: 1200, y: 500 }] } }]);
      const w = await openWorkspace(t.b, t.base, t.project);
      await t.b.waitFor(async () => (await overlayAlpha(t, w, 800, 520)) > 0, 'the ghost 20 px from the line');
      equal(await overlayAlpha(t, w, 800, 560), 0, 'nothing beyond the footprint');
      await t.b.eval(`document.querySelector('button.footprint-toggle').click(), 1`);
      await t.b.waitFor(async () => (await overlayAlpha(t, w, 800, 520)) === 0, 'the ghost hidden');
      await t.b.waitFor(async () => (await t.api.call('GET', '/api/me/settings')).show_footprint === false, 'the saved setting');
      // A label that hides its footprint has no ghost either.
      await t.b.eval(`document.querySelector('button.footprint-toggle').click(), 1`);
      await t.b.waitFor(async () => (await overlayAlpha(t, w, 800, 520)) > 0, 'the ghost again');
      await t.b.waitFor(async () => (await t.api.call('GET', '/api/me/settings')).show_footprint === true, 'the setting saved again');
      await changeLabel(t, 3, { show_footprint: false });
      await openWorkspace(t.b, t.base, t.project);
      await t.b.waitFor(async () => (await t.b.eval(`document.querySelector('canvas.annotations').dataset.count`)) === '1', 'the line');
      equal(await overlayAlpha(t, w, 800, 520), 0, 'no ghost for that label');
    } finally {
      await t.api.call('PUT', '/api/me/settings', {});
    }
  },
}, {
  name: 'the export dialog lists label footprints, changeable for one export',
  async run(t) {
    await t.api.setAnnotations(t.project, [{ label: 3, shape: { type: 'polyline', points: [{ x: 100, y: 100 }, { x: 300, y: 120 }] } }]);
    await t.b.goto(`${t.base}/projects/${t.project.id}`);
    await t.b.waitFor(() => t.b.eval(`!!document.querySelector('.grid li')`), 'the grid');
    await t.b.eval(`[...document.querySelectorAll('.actions button')].find((b) => b.textContent.includes('Export')).click(), 1`);
    await t.b.waitFor(() => t.b.eval(`!!document.querySelector('.export-dialog .footprints tbody tr')`), 'the footprint table');
    const rows = () => t.b.json(`[...document.querySelectorAll('.export-dialog .footprints tbody tr')].map((r) => [r.textContent.trim(), r.classList.contains('changed')])`);
    equal((await rows()).filter(([text]) => text.startsWith('line')).length, 1, 'a row for "line"');
    const state = () => t.b.json(`({
      bounds: !!document.querySelector('.export-dialog .footprint-bounds'),
      shapeColumn: [...document.querySelectorAll('.export-dialog .footprints th')].some((th) => th.textContent === 'Point shape'),
    })`);
    equal(await state(), { bounds: true, shapeColumn: false }, 'detect: the box option, no point shape');
    await t.b.eval(`(() => { const s = document.querySelector('.export-dialog select'); s.value = 'yolo-segment'; s.dispatchEvent(new Event('change', { bubbles: true })); return 1; })()`);
    await t.b.waitFor(async () => JSON.stringify(await state()) === JSON.stringify({ bounds: false, shapeColumn: true }), 'segment: the point shape, no box option');
    const width = `[...document.querySelectorAll('.export-dialog .footprints input')].find((i) => i.getAttribute('aria-label') === 'line line width')`;
    equal(await t.b.eval(`${width}.value`), '7', 'filled in from the label');
    await t.b.eval(`(() => { const i = ${width}; i.value = '12'; i.dispatchEvent(new Event('change', { bubbles: true })); return 1; })()`);
    await t.b.waitFor(() => t.b.eval(`!!document.querySelector('.export-dialog .footprints tr.changed button')`), 'the changed row');
    await t.b.eval(`document.querySelector('.export-dialog .footprints tr.changed button').click(), 1`);
    await t.b.waitFor(async () => (await t.b.eval(`${width}.value`)) === '7', 'reset to the label');
  },
});

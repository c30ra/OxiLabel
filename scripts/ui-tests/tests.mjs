// The browser-level flows of the annotation workspace. Each test gets a fresh project with the
// nine labels of NINE_LABELS (only "line", 3, rejects rectangles) and one 1600 × 1000 image.

import { writeFileSync } from 'node:fs';
import { join } from 'node:path';

import { KEY } from './bidi.mjs';
import { NINE_LABELS, box, openWorkspace, writeTestImage } from './rtag.mjs';

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
      equal(state.pieItems.filter((item) => item.enabled).map((item) => item.name), ['Select', 'Rectangle', 'Polygon', 'Points', 'Polyline', 'Brush', 'Circle', 'Tag'], 'enabled tools');
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

// Freehand polylines, single points, circles and the tool settings bar.
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
        c: 'Centre and radius* | Diameter',
        b: 'Paint* | Fuzzy fill | Brush* | Eraser | Stop at edgesShift',
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
    name: 'circles are drawn from the centre or by their diameter, and resized by their handle',
    async run(t) {
      const w = await openWorkspace(t.b, t.base, t.project);
      await t.b.key('c');
      await t.b.drag(w.at(500, 500), w.at(600, 500));
      let list = await saved(t, (list) => list.length === 1, 'the circle');
      near([list[0].shape.centre.x, list[0].shape.centre.y, list[0].shape.radius].map(Math.round), [500, 500, 100], 2 / t.scale(w), 'centre and radius');
      await clickOption(t, 'Diameter');
      await t.b.click(...w.at(200, 200));
      await t.b.click(...w.at(400, 200));
      list = await saved(t, (list) => list.length === 2, 'the diameter circle');
      near([list[1].shape.centre.x, list[1].shape.centre.y, list[1].shape.radius].map(Math.round), [300, 200, 100], 2 / t.scale(w), 'diameter circle');
      await t.b.key('v');
      await t.b.click(...w.at(500, 500));
      await t.b.drag(w.at(600, 500), w.at(650, 500));
      list = await saved(t, (list) => Math.round(list[0].shape.radius) >= 148, 'the radius change');
      near([Math.round(list[0].shape.radius)], [150], 2 / t.scale(w), 'radius');
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
    equal(await toolOptions(t), 'Paint* | Fuzzy fill | Brush | Eraser* | Stop at edgesShift', 'eraser mode');
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
    equal(await toolOptions(t), 'Paint | Fuzzy fill* | Brush* | Eraser | Contiguous* | Global', 'fuzzy fill options');
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
    await press('Add label');
    const label = await t.b.waitFor(async () => (await t.api.call('GET', `/api/projects/${t.project.id}/labels`)).find((l) => l.name === 'stain'), 'the new label');
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
    await t.b.key('G'); await waitAt(5);
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
    const zip = Buffer.from(await (await fetch(t.base + href)).arrayBuffer());
    equal(zip.subarray(0, 2).toString(), 'PK', 'zip signature');
    if (!zip.includes('images/train/') && !zip.includes('images/val/')) throw new Error('the zip has no images');
  },
});

export const LABELS = NINE_LABELS;

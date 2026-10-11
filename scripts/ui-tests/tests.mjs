// The browser-level flows of the annotation workspace. Each test gets a fresh project with the
// nine labels of NINE_LABELS (only "line", 3, rejects rectangles) and one 1600 × 1000 image.

import { KEY } from './bidi.mjs';
import { NINE_LABELS, box, openWorkspace } from './rtag.mjs';

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
    name: 'the tool pie keeps later tools in place, disabled',
    async run(t) {
      const w = await openWorkspace(t.b, t.base, t.project);
      await t.b.move(...w.at(800, 500));
      await t.b.click(...w.at(800, 500), { button: 2 });
      const state = await w.state();
      equal(state.pie, 'Tools', 'pie after a right-button tap without a last tool');
      equal(state.pieItems.filter((item) => item.enabled).map((item) => item.name), ['Select', 'Rectangle', 'Polygon', 'Points', 'Polyline'], 'enabled tools');
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
      await t.b.key('q');
      const state = await w.state();
      equal(state.pie, 'Rectangle options', 'pie');
      equal(state.pieItems.find((item) => item.name.startsWith('Square')).current, true, 'Square after picking it');
      await t.b.key(KEY.escape);
      equal((await w.state()).pie, null, 'pie after Esc');
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
      const [, second, third] = annotation.shape.points;
      if (Math.abs(second.y - 300) > 0.5) throw new Error(`second point not snapped: ${JSON.stringify(second)}`);
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

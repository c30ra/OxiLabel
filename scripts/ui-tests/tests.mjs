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
      equal(state.pieItems.filter((item) => item.enabled).map((item) => item.name), ['Select', 'Rectangle'], 'enabled tools');
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

export const LABELS = NINE_LABELS;

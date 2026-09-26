const { test } = require('node:test');
const assert = require('node:assert/strict');
const M = require('./model.js');

test('split keeps duration and source mapping', () => {
  const state = M.create(32);
  assert.equal(M.split(state, 12, 'second'), true);
  assert.equal(M.total(state.clips), 32);
  assert.equal(M.locate(state.clips, 12).source, 12);
  assert.equal(M.locate(state.clips, 25).source, 25);
});
test('split refuses zero-length boundary fragments', () => {
  const state = M.create();
  assert.equal(M.split(state, 0, 'start'), false);
  assert.equal(M.split(state, 32, 'end'), false);
  assert.equal(state.clips.length, 1);
});
test('ripple deletion skips removed source time', () => {
  const state = M.create(); M.split(state, 10, 'middle'); M.split(state, 20, 'last');
  assert.equal(M.removeClip(state, 'middle'), true);
  assert.equal(M.total(state.clips), 22);
  assert.equal(M.locate(state.clips, 10).source, 20);
  assert.equal(M.locate(state.clips, 15).source, 25);
});
test('cannot delete last clip or unknown clip', () => {
  const state = M.create();
  assert.equal(M.removeClip(state, 'video-1'), false);
  M.split(state, 10, 'second'); assert.equal(M.removeClip(state, 'missing'), false);
});
test('mixed changes undo and redo as a complete state', () => {
  const history = M.history(M.create());
  const next = M.clone(history.state); next.style.inset = 15; M.split(next, 4, 'second');
  history.commit(next); history.undo();
  assert.equal(history.state.style.inset, 8); assert.equal(history.state.clips.length, 1);
  history.redo(); assert.equal(history.state.style.inset, 15); assert.equal(history.state.clips.length, 2);
});
test('no-op creates no undo and new edit invalidates redo', () => {
  const history = M.history(M.create());
  assert.equal(history.commit(history.state), false); assert.equal(history.canUndo, false);
  const next = M.clone(history.state); next.auto = false; history.commit(next); history.undo();
  assert.equal(history.canRedo, true); next.style.inset = 10; history.commit(next);
  assert.equal(history.canRedo, false);
});
test('history clones caller state and clamps source lookup', () => {
  const initial = M.create(6), history = M.history(initial); initial.clips[0].out = 100;
  assert.equal(M.total(history.state.clips), 6);
  assert.equal(M.locate(history.state.clips, -2).source, 0);
  assert.equal(M.locate(history.state.clips, 10).source, 6);
});

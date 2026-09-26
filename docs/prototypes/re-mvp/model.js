/* Pure, in-memory interaction model. No Panzo project or source file is written. */
(function (root) {
  'use strict';
  const clone = value => JSON.parse(JSON.stringify(value));
  const clamp = (value, low, high) => Math.max(low, Math.min(high, value));
  const total = clips => clips.reduce((sum, clip) => sum + clip.out - clip.in, 0);
  function create(duration = 32) {
    return {
      clips: [{ id: 'video-1', in: 0, out: duration }],
      cameras: [{ id: 'camera-1', start: duration * .28, end: duration * .48, scale: 1.5, x: .5, y: .5 }],
      style: { background: 'mist', inset: 8, radius: 12, shadow: 30, cursor: true },
      auto: true,
    };
  }
  function locate(clips, tick) {
    let offset = 0;
    const target = clamp(tick, 0, total(clips));
    for (let i = 0; i < clips.length; i++) {
      const clip = clips[i], length = clip.out - clip.in;
      if (target < offset + length || i === clips.length - 1) {
        return { index: i, source: Math.min(clip.out, clip.in + target - offset), offset };
      }
      offset += length;
    }
    return null;
  }
  function split(state, tick, id) {
    const position = locate(state.clips, tick);
    if (!position) return false;
    const clip = state.clips[position.index];
    if (position.source - clip.in < 1 / 60 || clip.out - position.source < 1 / 60) return false;
    state.clips.splice(position.index, 1, { ...clip, out: position.source }, { ...clip, id, in: position.source });
    return true;
  }
  function removeClip(state, id) {
    if (state.clips.length <= 1) return false;
    const index = state.clips.findIndex(clip => clip.id === id);
    if (index < 0) return false;
    const start = total(state.clips.slice(0, index));
    const length = state.clips[index].out - state.clips[index].in;
    state.clips.splice(index, 1);
    // Prototype cameras use output time: contract for source-anchored production effects is in R3.
    const remap = t => t <= start ? t : t < start + length ? start : t - length;
    state.cameras = state.cameras.map(c => ({ ...c, start: remap(c.start), end: remap(c.end) }))
      .filter(c => c.end - c.start >= 1 / 60);
    return true;
  }
  function history(initial) {
    let state = clone(initial), undo = [], redo = [];
    return {
      get state() { return state; },
      get canUndo() { return undo.length > 0; },
      get canRedo() { return redo.length > 0; },
      commit(next) {
        if (JSON.stringify(state) === JSON.stringify(next)) return false;
        undo.push(clone(state)); if (undo.length > 100) undo.shift();
        state = clone(next); redo = []; return true;
      },
      undo() { if (!undo.length) return false; redo.push(state); state = undo.pop(); return true; },
      redo() { if (!redo.length) return false; undo.push(state); state = redo.pop(); return true; },
    };
  }
  const api = { clone, clamp, total, create, locate, split, removeClip, history };
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
  else root.PanzoModel = api;
})(typeof window !== 'undefined' ? window : this);

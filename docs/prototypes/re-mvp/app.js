/* R0 review prototype: deliberately no network, project writes or native recorder bindings. */
(() => {
  'use strict';
  const M = window.PanzoModel, $ = id => document.getElementById(id);
  let history = M.history(M.create()), draft = null, tick = 8, playing = false, selected = { type: 'camera', id: 'camera-1' };
  let nextId = 2, saved = JSON.stringify(history.state), drag = null, frameTime = 0, zoom = 1;
  let sourceUrl = null, backgroundUrl = null, sourceDuration = 32, sourceSize = [2560, 1440];
  let desiredSource = null, lastVideoSeek = 0, recordTimer = null, exportTimer = null;
  let recordStarted = 0, exportProgress = 0, activeView = 'editor';
  const state = () => draft || history.state;
  const duration = () => M.total(state().clips);
  const icon = name => `<svg aria-hidden="true"><use href="#i-${name}"/></svg>`;
  const format = seconds => {
    const ms = Math.round(Math.max(0, seconds) * 1000);
    return `${String(Math.floor(ms / 60000)).padStart(2, '0')}:${String(Math.floor(ms / 1000) % 60).padStart(2, '0')}.${String(ms % 1000).padStart(3, '0')}`;
  };
  const notify = message => { $('status').textContent = message; };
  const selectedItem = () => selected?.type === 'camera'
    ? state().cameras.find(c => c.id === selected.id) : state().clips.find(c => c.id === selected?.id);

  function commit(next, message) {
    const end = M.total(next.clips);
    next.cameras = next.cameras.map(c => ({ ...c, start: M.clamp(c.start, 0, end), end: M.clamp(c.end, 0, end) }))
      .filter(c => c.end - c.start >= 1 / 60);
    const changed = history.commit(next); draft = null;
    tick = M.clamp(tick, 0, duration());
    if (!selectedItem()) selected = { type: 'video', id: state().clips[0].id };
    renderAll(); if (changed && message) notify(message);
  }
  function edit(fn, message) { const next = M.clone(history.state); if (fn(next) !== false) commit(next, message); }
  function setPlaying(value) {
    if (value && tick >= duration() - .001) tick = 0;
    playing = value; frameTime = performance.now();
    $('play').innerHTML = icon(playing ? 'pause' : 'play');
    $('play').setAttribute('aria-label', playing ? '暂停' : '播放');
    // Source seeking is coordinated below; output-time playback works across deleted spans.
    $('source-video').pause();
  }
  function changeView(view) {
    activeView = view; setPlaying(false);
    $('editor').hidden = view !== 'editor'; $('recorder').hidden = view !== 'recorder';
    document.querySelectorAll('[data-view]').forEach(button => button.classList.toggle('active', button.dataset.view === view));
    if (view === 'editor') requestAnimationFrame(renderAll);
  }
  function renderControls() {
    $('undo').disabled = !history.canUndo; $('redo').disabled = !history.canRedo;
    $('save-state').textContent = JSON.stringify(history.state) === saved ? '原型更改仅保留在内存' : '有未保存的示例更改';
    const item = selectedItem(), isCamera = selected?.type === 'camera';
    const selectedIndex = item ? (isCamera ? state().cameras : state().clips).findIndex(c => c.id === item.id) + 1 : 0;
    $('selection-title').textContent = item ? (isCamera ? '镜头 ' : '视频片段 ') + String(selectedIndex).padStart(2, '0') : '未选中片段';
    $('scale-label').hidden = !isCamera; $('center-focus').hidden = !isCamera;
    $('delete').disabled = !item || (!isCamera && state().clips.length === 1);
    $('delete').setAttribute('aria-label', isCamera ? '删除选中镜头' : '删除选中视频片段');
    $('delete').title = (isCamera ? '删除选中镜头' : '删除选中视频片段并闭合空隙') + ' Delete';
    $('context-hint').textContent = isCamera ? '在画布中拖动焦点' : '裁剪不改写原始素材';
    for (const id of ['segment-start', 'segment-end', 'segment-scale']) $(id).disabled = !item;
    if (item) {
      $('segment-start').value = (isCamera ? item.start : item.in).toFixed(2);
      $('segment-end').value = (isCamera ? item.end : item.out).toFixed(2);
      $('segment-scale').value = isCamera ? item.scale.toFixed(1) : '1';
    }
    $('auto').checked = state().auto; $('show-cursor').checked = state().style.cursor;
    for (const key of ['inset', 'radius', 'shadow']) { $(key).value = state().style[key]; $(`${key}-value`).textContent = state().style[key]; }
    document.querySelectorAll('[data-background]').forEach(button => button.classList.toggle('selected', button.dataset.background === state().style.background));
  }
  function renderTimeline() {
    const length = duration(), container = $('track-content'), width = $('track-scroll').clientWidth * zoom;
    container.style.width = `${Math.max(1, width)}px`;
    const secondsPerLabel = length / Math.max(1, width / 86);
    const step = [1, 2, 5, 10, 15, 30, 60, 120, 300].find(s => s >= secondsPerLabel) || 600;
    let ticks = '';
    for (let t = 0; t < length; t += step) ticks += `<span class="tick" style="left:${t / length * 100}%">${format(t).slice(0, 5)}</span>`;
    $('ruler').innerHTML = ticks; $('ruler').setAttribute('aria-valuemax', length.toFixed(3));
    let offset = 0;
    $('video-track').innerHTML = state().clips.map((clip, index) => {
      const left = offset / length * 100; offset += clip.out - clip.in;
      return `<div class="clip ${selected?.id === clip.id ? 'selected' : ''}" data-clip="${clip.id}" tabindex="0" role="button" aria-label="视频片段 ${index + 1}" style="left:${left}%;width:${(clip.out - clip.in) / length * 100}%"><div class="filmstrip">${'<span class="film-frame"></span>'.repeat(30)}</div><span class="clip-title">${sourceUrl ? '本地视频' : '产品演示'} ${String(index + 1).padStart(2, '0')}</span><span class="handle left" data-edge="start"></span><span class="handle right" data-edge="end"></span></div>`;
    }).join('');
    $('camera-track').innerHTML = state().cameras.map((camera, index) => `<div class="camera ${selected?.id === camera.id ? 'selected' : ''}" data-camera="${camera.id}" tabindex="0" role="button" aria-label="镜头 ${index + 1}" style="left:${camera.start / length * 100}%;width:${(camera.end - camera.start) / length * 100}%;opacity:${state().auto ? 1 : .4}">${icon('camera')}<span>${camera.scale.toFixed(1)}×</span><span class="handle left" data-edge="start"></span><span class="handle right" data-edge="end"></span></div>`).join('');
  }
  function renderFrame() {
    const data = state(), length = duration();
    $('time-now').textContent = format(tick); $('time-total').textContent = format(length);
    $('ruler').setAttribute('aria-valuenow', tick.toFixed(3));
    $('playhead').style.left = `${tick / length * 100}%`;
    const item = selectedItem(), camera = data.auto ? data.cameras.find(c => tick >= c.start && tick < c.end) : null;
    const scale = camera ? camera.scale : 1;
    $('composition').className = `composition ${data.style.background}`;
    $('composition').style.backgroundImage = data.style.background === 'image' && backgroundUrl ? `url("${backgroundUrl}")` : '';
    $('composition').style.backgroundSize = 'cover'; $('composition').style.padding = `${data.style.inset}%`;
    $('screen-frame').style.borderRadius = `${data.style.radius}px`;
    $('screen-frame').style.boxShadow = `0 8px 24px rgb(0 0 0 / ${data.style.shadow / 100})`;
    const content = sourceUrl ? $('source-video') : $('sample-screen');
    content.style.transformOrigin = camera ? `${camera.x * 100}% ${camera.y * 100}%` : 'center';
    content.style.transform = `scale(${scale})`;
    $('focus-box').hidden = selected?.type !== 'camera' || !item || !data.auto;
    if (selected?.type === 'camera' && item) {
      $('focus-box').style.left = `${M.clamp(item.x * 100 - 15, 0, 70)}%`;
      $('focus-box').style.top = `${M.clamp(item.y * 100 - 20, 0, 60)}%`;
      $('focus-box').setAttribute('aria-valuenow', Math.round(item.x * 100));
    }
    $('demo-cursor').hidden = !data.style.cursor;
    $('demo-cursor').style.left = `${35 + Math.sin(tick * .22) * 19}%`;
    $('demo-cursor').style.top = `${46 + Math.cos(tick * .31) * 17}%`;
    if (sourceUrl) {
      const mapped = M.locate(data.clips, tick);
      desiredSource = Math.min(Math.max(0, sourceDuration - .001), mapped.source);
    }
  }
  function renderAll() { renderControls(); renderTimeline(); renderFrame(); }
  function positionAt(clientX) {
    const rect = $('track-content').getBoundingClientRect();
    return M.clamp((clientX - rect.left) / rect.width * duration(), 0, duration());
  }
  function beginDrag(event) {
    if (event.button !== 0) return;
    const camera = event.target.closest('[data-camera]'), clip = event.target.closest('[data-clip]');
    if (camera || clip) {
      selected = { type: camera ? 'camera' : 'video', id: camera ? camera.dataset.camera : clip.dataset.clip };
      renderControls();
      const edge = event.target.closest('[data-edge]')?.dataset.edge;
      if (!camera && !edge) { renderTimeline(); return; }
      drag = { type: selected.type, id: selected.id, edge, x: event.clientX, before: M.clone(history.state), length: duration(), width: $('track-content').getBoundingClientRect().width };
      draft = M.clone(history.state); setPlaying(false); renderTimeline();
    } else {
      drag = { type: 'scrub', resume: playing };
      $('ruler').focus({ preventScroll: true });
      setPlaying(false); tick = positionAt(event.clientX); renderFrame();
    }
    $('track-content').setPointerCapture(event.pointerId); event.preventDefault();
  }
  function moveDrag(event) {
    if (!drag) return;
    if (drag.type === 'scrub') { tick = positionAt(event.clientX); renderFrame(); return; }
    if (drag.type === 'focus') {
      const camera = draft.cameras.find(c => c.id === drag.id), rect = $('screen-frame').getBoundingClientRect();
      camera.x = M.clamp((event.clientX - rect.left) / rect.width, .15, .85);
      camera.y = M.clamp((event.clientY - rect.top) / rect.height, .2, .8); renderFrame(); return;
    }
    const delta = (event.clientX - drag.x) / drag.width * drag.length;
    draft = M.clone(drag.before);
    if (drag.type === 'camera') {
      const original = drag.before.cameras.find(c => c.id === drag.id), item = draft.cameras.find(c => c.id === drag.id);
      const before = draft.cameras.filter(c => c.id !== item.id && c.end <= original.start).at(-1);
      const after = draft.cameras.find(c => c.id !== item.id && c.start >= original.end);
      const low = before?.end || 0, high = after?.start || duration();
      if (drag.edge === 'start') item.start = M.clamp(original.start + delta, low, original.end - .1);
      else if (drag.edge === 'end') item.end = M.clamp(original.end + delta, original.start + .1, high);
      else { item.start = M.clamp(original.start + delta, low, high - (original.end - original.start)); item.end = item.start + original.end - original.start; }
    } else {
      const index = draft.clips.findIndex(c => c.id === drag.id), item = draft.clips[index];
      const original = drag.before.clips[index];
      if (drag.edge === 'start') item.in = M.clamp(original.in + delta, draft.clips[index - 1]?.out || 0, original.out - 1 / 60);
      else item.out = M.clamp(original.out + delta, original.in + 1 / 60, draft.clips[index + 1]?.in || sourceDuration);
      tick = Math.min(tick, duration());
    }
    renderControls(); renderTimeline(); renderFrame();
  }
  function endDrag(cancel = false) {
    if (!drag) return;
    const previous = drag; drag = null;
    if (previous.type === 'scrub') { if (previous.resume && !cancel) setPlaying(true); renderFrame(); return; }
    if (cancel) { draft = null; renderAll(); notify('已取消本次拖动'); }
    else if (draft) commit(draft, '已调整示例片段 · Ctrl+Z 可撤销');
  }
  $('track-content').addEventListener('pointerdown', beginDrag);
  window.addEventListener('pointermove', moveDrag);
  window.addEventListener('pointerup', () => endDrag());
  window.addEventListener('pointercancel', () => endDrag(true));
  window.addEventListener('blur', () => { endDrag(true); setPlaying(false); });
  $('focus-box').addEventListener('pointerdown', event => {
    if (event.button !== 0 || selected?.type !== 'camera') return;
    drag = { type: 'focus', id: selected.id }; draft = M.clone(history.state); setPlaying(false);
    event.currentTarget.setPointerCapture(event.pointerId); event.preventDefault();
  });

  $('play').onclick = () => setPlaying(!playing);
  $('home').onclick = () => { setPlaying(false); tick = 0; renderFrame(); };
  $('undo').onclick = () => { setPlaying(false); history.undo(); draft = null; tick = Math.min(tick, duration()); renderAll(); notify('已撤销示例更改'); };
  $('redo').onclick = () => { setPlaying(false); history.redo(); draft = null; tick = Math.min(tick, duration()); renderAll(); notify('已重做示例更改'); };
  $('save').onclick = () => { saved = JSON.stringify(history.state); renderControls(); notify('示例保存状态已更新；没有写入工程文件'); };
  $('split').onclick = () => { setPlaying(false); edit(next => M.split(next, tick, `video-${nextId++}`), '已模拟分割视频 · 正式能力将在 R3 接入'); };
  $('delete').onclick = () => {
    if ($('delete').disabled) return;
    setPlaying(false);
    edit(next => {
      if (selected.type === 'camera') next.cameras = next.cameras.filter(c => c.id !== selected.id);
      else return M.removeClip(next, selected.id);
    }, '已删除选中示例片段 · Ctrl+Z 可恢复');
  };
  $('add-camera').onclick = () => {
    setPlaying(false); const start = Math.min(tick, duration() - 1), end = Math.min(start + 3, duration());
    if (state().cameras.some(c => c.start < end && c.end > start)) { notify('此处与已有镜头重叠，请在空白范围添加'); return; }
    const id = `camera-${nextId++}`; selected = { type: 'camera', id };
    edit(next => { next.cameras.push({ id, start, end, scale: 1.5, x: .5, y: .5 }); next.cameras.sort((a, b) => a.start - b.start); }, '已添加示例镜头');
  };
  for (const id of ['segment-start', 'segment-end', 'segment-scale']) {
    $(id).addEventListener('change', () => {
      const value = Number($(id).value); if (!Number.isFinite(value) || !selectedItem()) { renderControls(); return; }
      edit(next => {
        if (selected.type === 'camera') {
          const item = next.cameras.find(c => c.id === selected.id);
          if (id === 'segment-scale') item.scale = M.clamp(value, 1, 3);
          else if (id === 'segment-start') item.start = M.clamp(value, 0, item.end - .1);
          else item.end = M.clamp(value, item.start + .1, duration());
          if (next.cameras.some(c => c.id !== item.id && c.start < item.end && c.end > item.start)) { notify('时间范围与其他镜头重叠'); renderControls(); return false; }
          next.cameras.sort((a, b) => a.start - b.start);
        } else {
          const index = next.clips.findIndex(c => c.id === selected.id), item = next.clips[index];
          if (id === 'segment-start') item.in = M.clamp(value, next.clips[index - 1]?.out || 0, item.out - 1 / 60);
          else item.out = M.clamp(value, item.in + 1 / 60, next.clips[index + 1]?.in || sourceDuration);
        }
      }, '已更新示例数值');
    });
  }
  $('center-focus').onclick = () => edit(next => { const item = next.cameras.find(c => c.id === selected?.id); if (!item) return false; item.x = item.y = .5; }, '焦点已居中');
  $('focus-box').addEventListener('keydown', event => {
    const delta = { ArrowLeft: [-.01, 0], ArrowRight: [.01, 0], ArrowUp: [0, -.01], ArrowDown: [0, .01] }[event.key];
    if (!delta) return; event.preventDefault();
    edit(next => { const item = next.cameras.find(c => c.id === selected?.id); item.x = M.clamp(item.x + delta[0], .15, .85); item.y = M.clamp(item.y + delta[1], .2, .8); }, '已微调焦点');
  });
  $('ruler').addEventListener('keydown', event => {
    if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return;
    event.preventDefault(); setPlaying(false);
    tick = event.key === 'Home' ? 0 : event.key === 'End' ? duration() : M.clamp(tick + (event.key === 'ArrowLeft' ? -1 : 1) / 60, 0, duration()); renderFrame();
  });
  for (const key of ['inset', 'radius', 'shadow']) {
    $(key).addEventListener('input', () => { draft ||= M.clone(history.state); draft.style[key] = Number($(key).value); $(`${key}-value`).textContent = $(key).value; renderFrame(); });
    $(key).addEventListener('change', () => { if (draft) commit(draft, '已调整示例外观'); });
  }
  $('auto').onchange = () => edit(next => { next.auto = $('auto').checked; }, '镜头效果开关仅影响预览，不删除镜头');
  $('show-cursor').onchange = () => edit(next => { next.style.cursor = $('show-cursor').checked; }, sourceUrl ? '只切换原型光标；不能移除源视频内的光标' : '已切换示例光标');
  document.querySelectorAll('[data-background]').forEach(button => button.onclick = () => edit(next => { next.style.background = button.dataset.background; }, '已调整示例背景'));
  document.querySelectorAll('[data-background-tab]').forEach(button => button.onclick = () => {
    document.querySelectorAll('[data-background-tab]').forEach(b => b.classList.toggle('selected', b === button));
    document.querySelector('.swatches').hidden = button.dataset.backgroundTab === 'image'; $('choose-background').hidden = button.dataset.backgroundTab !== 'image';
  });
  $('choose-background').onclick = () => $('background-file').click();
  $('background-file').onchange = () => {
    const file = $('background-file').files[0]; if (!file) return;
    if (backgroundUrl) URL.revokeObjectURL(backgroundUrl); backgroundUrl = URL.createObjectURL(file);
    edit(next => { next.style.background = 'image'; }, '图片仅在当前原型页面中使用，不上传');
  };
  function setZoom(value, anchor = .5) {
    const scroll = $('track-scroll'), oldWidth = scroll.clientWidth * zoom, point = scroll.scrollLeft + anchor * scroll.clientWidth;
    zoom = M.clamp(value, 1, 4); $('zoom').value = zoom; renderTimeline();
    scroll.scrollLeft = point / oldWidth * scroll.clientWidth * zoom - anchor * scroll.clientWidth; renderFrame();
  }
  $('zoom').oninput = () => setZoom(Number($('zoom').value));
  $('zoom-out').onclick = () => setZoom(zoom - .25); $('zoom-in').onclick = () => setZoom(zoom + .25); $('fit').onclick = () => setZoom(1);
  $('track-scroll').addEventListener('wheel', event => {
    if (!event.ctrlKey) return; event.preventDefault();
    const rect = event.currentTarget.getBoundingClientRect(); setZoom(zoom + (event.deltaY > 0 ? -.25 : .25), (event.clientX - rect.left) / rect.width);
  }, { passive: false });
  $('toggle-inspector').onclick = () => { $('inspector').hidden = !$('inspector').hidden; requestAnimationFrame(renderAll); };
  document.querySelectorAll('[data-view]').forEach(button => button.onclick = () => changeView(button.dataset.view));
  $('back-recorder').onclick = () => changeView('recorder'); $('open-example').onclick = () => changeView('editor');
  document.querySelectorAll('[data-source]').forEach(button => button.onclick = () => {
    document.querySelectorAll('[data-source]').forEach(b => b.classList.toggle('selected', b === button));
    $('source-name').textContent = button.dataset.source === 'window' ? '示例应用窗口' : '主显示器';
    $('source-spec').textContent = button.dataset.source === 'window' ? '1920 × 1080 · 示例尺寸 · 60 FPS' : '2560 × 1440 · 原始尺寸 · 60 FPS';
  });
  $('record').onclick = () => {
    if (!recordTimer) {
      recordStarted = Date.now(); $('record').classList.add('recording'); $('record-label').textContent = '停止模拟录制'; $('record-time').hidden = false;
      $('recorder-hint').textContent = '模拟录制中 · 不访问你的屏幕';
      recordTimer = setInterval(() => { $('record-time').textContent = format((Date.now() - recordStarted) / 1000).slice(0, 5); }, 100);
    } else {
      clearInterval(recordTimer); recordTimer = null; $('record').disabled = true; $('record-label').textContent = '正在模拟收尾…';
      setTimeout(() => { $('record').disabled = false; $('record').classList.remove('recording'); $('record-label').textContent = '模拟开始录制'; $('record-time').hidden = true; $('recorder-hint').textContent = '原型仅演示录制流程，不访问屏幕或麦克风'; changeView('editor'); notify('录制流程演示完成；打开的仍是示例工程'); }, 500);
    }
  };
  function openExport() {
    setPlaying(false); exportProgress = 0;
    $('export-settings').hidden = false; $('export-progress').hidden = true;
    $('export-start').disabled = false; $('export-start').innerHTML = `${icon('export')}模拟导出`;
    $('export-cancel').textContent = '取消'; $('export-duration').textContent = `${format(duration()).slice(0, 5)} · H.264 · 无音频`;
    $('export-dialog').showModal();
  }
  $('export-open').onclick = openExport; $('review-export').onclick = openExport;
  $('export-start').onclick = () => {
    $('export-settings').hidden = true; $('export-progress').hidden = false; $('export-start').disabled = true;
    exportTimer = setInterval(() => {
      exportProgress = Math.min(100, exportProgress + 3);
      $('export-fill').style.width = `${exportProgress}%`; $('export-percent').textContent = `${exportProgress}%`;
      $('export-progress-text').textContent = exportProgress === 100 ? '演示完成，没有生成视频文件' : '正在模拟导出…';
      if (exportProgress === 100) { clearInterval(exportTimer); exportTimer = null; $('export-cancel').textContent = '完成演示'; }
    }, 80);
  };
  $('export-dialog').addEventListener('close', () => { clearInterval(exportTimer); exportTimer = null; $('export-fill').style.width = '0%'; });
  $('help-open').onclick = () => $('help-dialog').showModal();
  $('load-video').onclick = () => $('video-file').click();
  $('video-file').onchange = () => {
    const file = $('video-file').files[0]; if (!file) return;
    setPlaying(false); if (sourceUrl) URL.revokeObjectURL(sourceUrl);
    sourceUrl = URL.createObjectURL(file); $('source-video').src = sourceUrl;
    $('source-video').hidden = false; $('sample-screen').hidden = true;
    $('project-subtitle').textContent = file.name; $('project-subtitle').title = file.name;
    $('preview-note').textContent = '本地素材 · 不上传 · 不修改原文件';
  };
  $('source-video').addEventListener('loadedmetadata', () => {
    const video = $('source-video'); if (!Number.isFinite(video.duration) || video.duration <= 0) { notify('此视频无法取得有效时长'); return; }
    sourceDuration = video.duration; sourceSize = [video.videoWidth, video.videoHeight];
    history = M.history(M.create(sourceDuration)); draft = null; tick = Math.min(1, sourceDuration); selected = { type: 'camera', id: 'camera-1' }; saved = JSON.stringify(history.state);
    $('preview-size').textContent = `${sourceSize[0]} × ${sourceSize[1]}`;
    $('export-resolution').options[0].textContent = `原始尺寸 · ${sourceSize[0]} × ${sourceSize[1]}`;
    renderAll(); notify('已载入本地视频；浏览器预览不代表正式应用性能');
  });
  $('source-video').addEventListener('error', () => notify('浏览器无法解码此素材；请换一个 MP4，原文件未被修改'));
  window.addEventListener('keydown', event => {
    const isText = /^(INPUT|TEXTAREA|SELECT)$/.test(event.target.tagName), modal = document.querySelector('dialog[open]');
    if (event.key === 'Escape' && drag) { endDrag(true); event.preventDefault(); return; }
    if (isText || modal || activeView !== 'editor') return;
    if (event.code === 'Space' && event.target.tagName !== 'BUTTON' && event.target.id !== 'focus-box') { event.preventDefault(); setPlaying(!playing); }
    if (event.ctrlKey && event.key.toLowerCase() === 'z') { event.preventDefault(); $('undo').click(); }
    if (event.ctrlKey && event.key.toLowerCase() === 'y') { event.preventDefault(); $('redo').click(); }
    if (event.ctrlKey && event.key.toLowerCase() === 's') { event.preventDefault(); $('save').click(); }
    if (event.key === 'Delete') { event.preventDefault(); $('delete').click(); }
  });
  function animate(now) {
    if (playing && !drag && activeView === 'editor') {
      tick = Math.min(duration(), tick + Math.min(.1, (now - frameTime) / 1000)); renderFrame();
      if (tick >= duration()) setPlaying(false);
    }
    frameTime = now;
    const video = $('source-video');
    if (sourceUrl && video.readyState >= 1 && !video.seeking && desiredSource !== null && Math.abs(video.currentTime - desiredSource) > .015 && now - lastVideoSeek >= 33) {
      video.currentTime = desiredSource; lastVideoSeek = now;
    }
    requestAnimationFrame(animate);
  }
  new ResizeObserver(() => { if (activeView === 'editor') { renderTimeline(); renderFrame(); } }).observe($('track-scroll'));
  window.addEventListener('pagehide', () => { if (sourceUrl) URL.revokeObjectURL(sourceUrl); if (backgroundUrl) URL.revokeObjectURL(backgroundUrl); clearInterval(recordTimer); clearInterval(exportTimer); });
  renderAll(); requestAnimationFrame(animate);
})();

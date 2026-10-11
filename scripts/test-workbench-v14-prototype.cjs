// Tests only an isolated headless Chromium launched for this local prototype.
// Never connects to the user's browser/profile. No external packages or network assets.
const fs = require('node:fs');
const path = require('node:path');
const { spawn } = require('node:child_process');
const { pathToFileURL } = require('node:url');
const assert = require('node:assert/strict');
const root = path.resolve(__dirname, '..');
const runId = `workbench-v14-${new Date().toISOString().replace(/[:.]/g, '-')}`;
const output = path.join(root, 'docs', 'acceptance', 'workbench-v14-design', runId);
const profile = fs.mkdtempSync(path.join(root, '.tmp', 'workbench-v14-browser-'));
fs.mkdirSync(output, { recursive: true });
const chrome = ['C:/Program Files/Google/Chrome/Application/chrome.exe', 'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe'].find(fs.existsSync);
if (!chrome) throw new Error('No local Chromium available; prototype browser checks are NotRun');
const child = spawn(chrome, ['--headless=new', '--disable-gpu', '--remote-debugging-port=0', `--user-data-dir=${profile}`, '--no-first-run', '--no-default-browser-check', '--disable-extensions', '--disable-background-networking', 'about:blank'], { windowsHide: true, stdio: ['ignore', 'ignore', 'pipe'] });
const errors = [], checks = [];
let browserSocket, pageSocket;
const wait = ms => new Promise(resolve => setTimeout(resolve, ms));

async function connect(url) {
  const socket = new WebSocket(url); const pending = new Map(); let nextId = 0;
  await new Promise((resolve, reject) => { socket.addEventListener('open', resolve, { once: true }); socket.addEventListener('error', reject, { once: true }); });
  socket.addEventListener('message', event => {
    const message = JSON.parse(event.data);
    if (message.method === 'Runtime.exceptionThrown') errors.push(message.params.exceptionDetails);
    if (!message.id || !pending.has(message.id)) return;
    const { resolve, reject, timer } = pending.get(message.id); clearTimeout(timer); pending.delete(message.id);
    if (message.error) reject(new Error(JSON.stringify(message.error))); else resolve(message.result);
  });
  return {
    socket,
    send(method, params = {}) {
      return new Promise((resolve, reject) => {
        const id = ++nextId, timer = setTimeout(() => { pending.delete(id); reject(new Error(`CDP timeout: ${method}`)); }, 15000);
        pending.set(id, { resolve, reject, timer }); socket.send(JSON.stringify({ id, method, params }));
      });
    },
  };
}
async function evaluate(expression) {
  const response = await pageSocket.send('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true });
  if (response.exceptionDetails) throw new Error(JSON.stringify(response.exceptionDetails));
  return response.result.value;
}
async function point(selector) {
  const rect = await evaluate(`(() => { const el = document.querySelector(${JSON.stringify(selector)}); if (!el) return null; const r=el.getBoundingClientRect(); return {x:r.x+r.width/2,y:r.y+r.height/2,width:r.width,height:r.height}; })()`);
  assert.ok(rect?.width > 0 && rect?.height > 0, `visible target: ${selector}`); return rect;
}
async function click(selector) {
  const p = await point(selector);
  await pageSocket.send('Input.dispatchMouseEvent', { type: 'mousePressed', x: p.x, y: p.y, button: 'left', clickCount: 1 });
  await pageSocket.send('Input.dispatchMouseEvent', { type: 'mouseReleased', x: p.x, y: p.y, button: 'left', clickCount: 1 });
  await wait(80);
}
async function drag(selector, pixels) {
  const p = await point(selector);
  await pageSocket.send('Input.dispatchMouseEvent', { type: 'mousePressed', x: p.x, y: p.y, button: 'left', clickCount: 1 });
  for (let step = 1; step <= 16; step++) {
    await pageSocket.send('Input.dispatchMouseEvent', { type: 'mouseMoved', x: p.x + pixels * step / 16, y: p.y, button: 'left', buttons: 1 }); await wait(8);
  }
  await pageSocket.send('Input.dispatchMouseEvent', { type: 'mouseReleased', x: p.x + pixels, y: p.y, button: 'left', clickCount: 1 }); await wait(80);
}
async function space() {
  await pageSocket.send('Input.dispatchKeyEvent', { type: 'keyDown', key: ' ', code: 'Space', windowsVirtualKeyCode: 32 });
  await pageSocket.send('Input.dispatchKeyEvent', { type: 'keyUp', key: ' ', code: 'Space', windowsVirtualKeyCode: 32 });
  await wait(70);
}
async function screenshot(name) {
  await evaluate('document.querySelector("#toast").hidden=true'); await wait(220);
  const image = await pageSocket.send('Page.captureScreenshot', { format: 'png', captureBeyondViewport: false });
  fs.writeFileSync(path.join(output, `${name}.png`), Buffer.from(image.data, 'base64'));
}
async function check(name, fn) { await fn(); checks.push({ name, status: 'Pass' }); console.log(`PASS ${name}`); }

(async()=>{
 try{
  const browserUrl=await new Promise((resolve,reject)=>{
   let text='';const timer=setTimeout(()=>reject(new Error('Browser launch timeout')),15000);
   child.stderr.on('data',chunk=>{text+=chunk.toString();const match=text.match(/DevTools listening on (ws:\/\/[^\s]+)/);if(match){clearTimeout(timer);resolve(match[1]);}});
   child.on('error',reject);
  });
  browserSocket=await connect(browserUrl);
  const created=await browserSocket.send('Target.createTarget',{url:'about:blank'});
  const targets=await(await fetch('http://127.0.0.1:'+new URL(browserUrl).port+'/json/list')).json();
  pageSocket=await connect(targets.find(t=>t.id===created.targetId).webSocketDebuggerUrl);
  await pageSocket.send('Runtime.enable');await pageSocket.send('Page.enable');
  await pageSocket.send('Emulation.setDeviceMetricsOverride',{width:1440,height:900,deviceScaleFactor:1,mobile:false});
  await pageSocket.send('Page.navigate',{url:pathToFileURL(path.join(root,'docs/prototypes/workbench-v14/index.html')).href});
  await wait(600);
  await check('video-shell-alignment-and-native-theme',async()=>{
   assert.equal(await evaluate('S.page'),'video');
   const b=await evaluate('(()=>{const a=document.querySelector(".inspector").getBoundingClientRect(),t=document.querySelector(".timeline").getBoundingClientRect(),n=document.querySelector(".workspaces").getBoundingClientRect();return {bottom:a.bottom,top:t.top,center:n.left+n.width/2}})()');
   assert.ok(Math.abs(b.bottom-b.top)<=1);assert.ok(Math.abs(b.center-720)<=1);assert.equal(await evaluate('document.querySelector(".brand img").naturalWidth>0'),true);
   assert.equal(await evaluate('(()=>{let b=document.querySelector("[data-action=video-crop]").getBoundingClientRect(),p=document.querySelector(".inspector-body").getBoundingClientRect();return b.top>=p.top&&b.bottom<=p.bottom})()'),true,'crop action must be initially visible');
   await screenshot('01-video-dark');
  });
  await check('selected-clips-route-properties-split-delete-undo',async()=>{
   await click('[data-action="clip:a1"]');assert.ok(await evaluate('document.querySelector(".selection-heading").textContent.includes("旁白")'));
   await click('[data-action="clip:v1"]');const n=await evaluate('S.clips.length');await click('[data-action="split"]');assert.equal(await evaluate('S.clips.length'),n+1);
   await click('[data-action="delete-clip"]');assert.equal(await evaluate('S.clips.length'),n);await click('[data-action="undo"]');assert.equal(await evaluate('S.clips.length'),n+1);await click('[data-action="undo"]');assert.equal(await evaluate('S.clips.length'),n);
  });
  await check('preview-wheel-pan-center-and-inspector-resize',async()=>{
   const p=await point('#canvas');await pageSocket.send('Input.dispatchMouseEvent',{type:'mouseWheel',x:p.x,y:p.y,deltaX:0,deltaY:-100,modifiers:2});await wait(60);assert.ok(await evaluate('S.zoom>1'));
   await drag('#canvas',70);assert.ok(await evaluate('S.pan.x>40'));await click('[data-action="fit"]');assert.equal(await evaluate('S.zoom'),1);assert.equal(await evaluate('S.pan.x'),0);
   const before=await evaluate('document.querySelector(".inspector").offsetWidth');await drag('.splitter-vertical',-30);assert.ok((await evaluate('document.querySelector(".inspector").offsetWidth'))>before);
   await evaluate('document.documentElement.style.removeProperty("--inspector");fitCanvas()');
  });
  await check('fixed-voice-controls-and-hidden-task-return',async()=>{
   await click('[data-action="video-tab:audio"]');await screenshot('02-audio-dark');
   await click('[data-action="start-recording"]');await click('[data-page="image"]');assert.equal(await evaluate('document.querySelector("#background-task").hidden'),false);
   await click('[data-action="return-task"]');assert.equal(await evaluate('S.page'),'video');assert.equal(await evaluate('S.videoTab'),'audio');
   await click('[data-action="stop-recording"]');assert.equal(await evaluate('S.recording'),false);
  });
  await check('subtitle-workflow-and-independent-plugin-state',async()=>{
   await click('[data-action="video-tab:subtitle"]');
   assert.equal(await evaluate('(()=>{let b=document.querySelector("[data-value=subtitle-text]").getBoundingClientRect(),p=document.querySelector(".inspector-body").getBoundingClientRect();return b.top>=p.top&&b.bottom<=p.bottom})()'),true,'selected subtitle editing must be initially visible');
   await screenshot('03-subtitle-dark');
   await click('[data-action="ai-subtitle"]');await click('[data-action="plugins"]');assert.equal(await evaluate('document.querySelector("[data-action=toggle-plugin]").disabled'),true);
   await click('[data-action="close-dialog"]');await wait(2400);
   await evaluate('document.querySelector("[data-value=subtitle-text]").value="新的字幕内容"');
   await click('[data-action="apply-subtitle"]');assert.equal(await evaluate('S.subtitles[S.selectedSubtitle]'),'新的字幕内容');
   await click('[data-action="plugins"]');await click('[data-action="toggle-plugin"]');assert.equal(await evaluate('S.plugin'),false);await screenshot('09-plugins-dark');await click('[data-action="close-dialog"]');
   assert.ok(await evaluate('document.querySelector(".feature-off").textContent.includes("已停用")'));await click('[data-action="plugins"]');await click('[data-action="toggle-plugin"]');await click('[data-action="close-dialog"]');
  });
  await check('timeline-keeps-capped-rows-and-compact-hides-preview',async()=>{
   const p=await point('.timeline-divider');const before=await evaluate('document.querySelector(".track-row.video").offsetHeight');
   await pageSocket.send('Input.dispatchMouseEvent',{type:'mousePressed',x:p.x,y:p.y,button:'left',clickCount:1});
   await pageSocket.send('Input.dispatchMouseEvent',{type:'mouseMoved',x:p.x,y:p.y-100,button:'left',buttons:1});
   await pageSocket.send('Input.dispatchMouseEvent',{type:'mouseReleased',x:p.x,y:p.y-100,button:'left',clickCount:1});
   assert.equal(await evaluate('document.querySelector(".track-row.video").offsetHeight'),before);
   await evaluate('S.timelineHeight=168;render()');assert.equal(await evaluate('getComputedStyle(document.querySelector(".filmframes")).display'),'none');await screenshot('10-timeline-compact');
   await evaluate('S.timelineHeight=null;render()');
  });
  await check('image-context-crop-and-inline-recognition',async()=>{
   await click('[data-page="image"]');await click('[data-action="tool:rectangle"]');await click('.image-paper');await screenshot('04-image-dark');
   await click('[data-action="tool:crop"]');await click('[data-action="ratio:1:1"]');await drag('.crop-handle.se',-25);await screenshot('05-image-crop-dark');
   await click('[data-action="apply-crop"]');await click('[data-action="tool:ocr"]');await click('[data-action="recognize"]');await wait(2000);
   assert.ok(await evaluate('document.querySelector(".ocr-result").textContent.includes("灵感")'));assert.equal(await evaluate('document.querySelector("#dialog").open'),false);await screenshot('06-image-ocr-dark');
  });
  await check('collage-aligned-grid-ratio-and-preview-transform',async()=>{
   await click('[data-action="tool:collage"]');await click('[data-action="cell-ratio:4:3"]');
   const rects=await evaluate('[...document.querySelectorAll(".collage-cell")].map(e=>{let r=e.getBoundingClientRect();return {left:r.left,top:r.top,width:r.width,height:r.height}})');
   assert.equal(rects.length,4);assert.ok(Math.abs(rects[0].width/rects[0].height-4/3)<.02,'cell ratio must affect actual geometry');assert.ok(Math.abs(rects[0].left-rects[2].left)<1);assert.ok(Math.abs(rects[1].left-rects[3].left)<1);assert.equal(await evaluate('S.collageRatio'),'4:3');
   const p=await point('.collage-cell');await pageSocket.send('Input.dispatchMouseEvent',{type:'mouseWheel',x:p.x,y:p.y,deltaX:0,deltaY:-120});await drag('.collage-cell',16);
   assert.ok(await evaluate('S.cells[0].z>1&&S.cells[0].x>0'));
   const sizes=await evaluate('[...document.querySelectorAll(".filmstrip button")].map(b=>({w:b.offsetWidth,h:b.offsetHeight}))');assert.ok(sizes.every(s=>s.w===75&&s.h===54));await screenshot('07-collage-dark');
  });
  await check('asset-search-preview-and-editor-handoff',async()=>{
   await click('[data-page="assets"]');assert.equal(await evaluate('document.querySelector(".export-button").hidden'),true);await screenshot('08-assets-dark');
   await evaluate('(()=>{const input=document.querySelector("[data-value=asset-search]");input.value="封面";input.dispatchEvent(new Event("input",{bubbles:true}))})()');assert.equal(await evaluate('document.querySelectorAll(".asset-card").length'),1);
   await click('.asset-card');await click('[data-action="use-asset"]');assert.equal(await evaluate('S.page'),'image');
  });
  await check('settings-dialog-draft-and-theme',async()=>{
   await click('[data-action="settings"]');await screenshot('11-settings-dark');await click('[data-action="settings:vision"]');
   await evaluate('(()=>{let el=document.querySelector("[data-value=model]");el.value="my-vision-model";el.dispatchEvent(new Event("input",{bubbles:true}))})()');
   await click('[data-action="settings:general"]');await click('[data-action="settings:vision"]');assert.equal(await evaluate('document.querySelector("[data-value=model]").value'),'my-vision-model');
   await click('[data-action="close-dialog"]');await click('[data-page="video"]');await click('[data-action="video-tab:clip"]');await click('[data-action="clip:v1"]');await click('#theme');await screenshot('12-video-light');
  });
  await check('record-and-export-dialogs',async()=>{await click('[data-action="record"]');await screenshot('13-record-light');await click('[data-action="close-dialog"]');await click('[data-action="export"]');await screenshot('14-export-light');await click('[data-action="close-dialog"]');});
  await check('compact-960x720-no-global-overflow-and-visible-actions',async()=>{
   await pageSocket.send('Emulation.setDeviceMetricsOverride',{width:960,height:720,deviceScaleFactor:1,mobile:false});await evaluate('S.query="";S.bin=true;S.timelineHeight=null;render()');
   for(const p of ['video','image','assets']){
    await click('[data-page="'+p+'"]');const geo=await evaluate('({sw:document.documentElement.scrollWidth,w:innerWidth,b:document.querySelector("#app").getBoundingClientRect().bottom,h:innerHeight})');assert.ok(geo.sw<=geo.w);assert.ok(geo.b<=geo.h);
   }
   await click('[data-page="video"]');
   await click('[data-action=bin]');assert.equal(await evaluate('getComputedStyle(document.querySelector(".project-bin")).display'),'flex');await click('[data-action=bin]');
   await click('[data-action="video-tab:audio"]');
   const bounds=await evaluate('(()=>{let b=document.querySelector("[data-action=start-recording]").getBoundingClientRect(),p=document.querySelector(".inspector").getBoundingClientRect();return {b:b.bottom,t:b.top,p:p.bottom,pt:p.top}})()');assert.ok(bounds.b<=bounds.p&&bounds.t>=bounds.pt);await screenshot('15-compact-audio-light');
  });
  assert.equal(errors.length,0,JSON.stringify(errors));
  const report={runId,status:'Pass',scope:'design-prototype-not-native-app',checks,javascriptErrors:errors};
  fs.writeFileSync(path.join(output,'prototype-tests.json'),JSON.stringify(report,null,2));
  const previews=path.join(root,'docs/prototypes/workbench-v14/previews');fs.mkdirSync(previews,{recursive:true});
  for(const f of fs.readdirSync(output).filter(f=>f.endsWith('.png')||f.endsWith('.json')))fs.copyFileSync(path.join(output,f),path.join(previews,f));
  console.log('Prototype evidence: '+output);
 }catch(error){fs.writeFileSync(path.join(output,'prototype-tests.json'),JSON.stringify({runId,status:'Fail',checks,error:error.stack,javascriptErrors:errors},null,2));console.error(error);process.exitCode=1;
 }finally{pageSocket?.socket.close();if(browserSocket){await browserSocket.send('Browser.close').catch(()=>{});browserSocket.socket.close();}if(child.exitCode===null)child.kill();}
})();

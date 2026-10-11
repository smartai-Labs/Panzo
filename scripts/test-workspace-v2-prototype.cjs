// Tests only an isolated headless Chromium launched for this local prototype.
// Never connects to the user's browser/profile. No external packages or network assets.
const fs = require('node:fs');
const path = require('node:path');
const { spawn } = require('node:child_process');
const { pathToFileURL } = require('node:url');
const assert = require('node:assert/strict');
const root = path.resolve(__dirname, '..');
const runId = `workspace-v2-${new Date().toISOString().replace(/[:.]/g, '-')}`;
const output = path.join(root, 'docs', 'acceptance', 'workspace-v2-design', runId);
const profile = fs.mkdtempSync(path.join(root, '.tmp', 'workspace-v2-browser-'));
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
  await evaluate('document.querySelector("#toast").classList.remove("show")'); await wait(220);
  const image = await pageSocket.send('Page.captureScreenshot', { format: 'png', captureBeyondViewport: false });
  fs.writeFileSync(path.join(output, `${name}.png`), Buffer.from(image.data, 'base64'));
}
async function check(name, fn) { await fn(); checks.push({ name, status: 'Pass' }); console.log(`PASS ${name}`); }

(async () => {
  try {
    const browserUrl = await new Promise((resolve, reject) => {
      let text = ''; const timer = setTimeout(() => reject(new Error('Isolated Chromium launch timed out')), 15000);
      child.stderr.on('data', chunk => { text += chunk.toString(); const match = text.match(/DevTools listening on (ws:\/\/[^\s]+)/); if (match) { clearTimeout(timer); resolve(match[1]); } });
      child.on('error', error => { clearTimeout(timer); reject(error); });
    });
    browserSocket = await connect(browserUrl);
    const created = await browserSocket.send('Target.createTarget', { url: 'about:blank' });
    const port = new URL(browserUrl).port;
    const targets = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
    const target = targets.find(t => t.id === created.targetId); assert.ok(target);
    pageSocket = await connect(target.webSocketDebuggerUrl); await pageSocket.send('Runtime.enable'); await pageSocket.send('Page.enable');
    await pageSocket.send('Emulation.setDeviceMetricsOverride', { width: 1440, height: 900, deviceScaleFactor: 1, mobile: false });
    await pageSocket.send('Page.navigate', { url: pathToFileURL(path.join(root, 'docs/prototypes/workspace-v2/index.html')).href }); await wait(700);

    await check('empty-image-workspace', async()=>{assert.ok(await evaluate('!!document.querySelector(".empty-stage")'));assert.equal(await evaluate('document.querySelectorAll("#ocr-result").length'),0);await screenshot('image-empty-light');});
    await check('capture-annotation-and-undo',async()=>{
      await click('[data-action="capture"]'); await wait(150); await click('[data-action="tool:rect"]');
      const p=await point('#annotation-layer');
      await pageSocket.send('Input.dispatchMouseEvent',{type:'mousePressed',x:p.x-130,y:p.y-50,button:'left',clickCount:1});
      await pageSocket.send('Input.dispatchMouseEvent',{type:'mouseMoved',x:p.x+65,y:p.y+20,button:'left',buttons:1});
      await pageSocket.send('Input.dispatchMouseEvent',{type:'mouseReleased',x:p.x+65,y:p.y+20,button:'left',clickCount:1});
      await wait(80);assert.equal(await evaluate('S.docs[0].annotations.length'),1);
      await click('[data-page="settings"]');await click('[data-page="image"]');assert.equal(await evaluate('S.docs[0].annotations.length'),1);
      await click('[data-action="undo"]');assert.equal(await evaluate('S.docs[0].annotations.length'),0);
    });
    await check('background-recognition-stays-with-source',async()=>{
      await click('[data-action="recognize"]');await click('[data-action="capture"]');await click('[data-page="video"]');await wait(1900);await click('[data-page="image"]');
      assert.equal(await evaluate('S.active'),2);assert.equal(await evaluate('S.docs[1].result'),'');assert.ok((await evaluate('S.docs[0].result')).includes('演示识别结果'));
      await click('.image-tabs [data-doc="1"]');await evaluate('document.querySelector("#ocr-result").value="人工校对的结果";document.querySelector("#ocr-result").dispatchEvent(new Event("input",{bubbles:true}))');
      await click('[data-page="library"]');await click('[data-page="image"]');assert.equal(await evaluate('document.querySelector("#ocr-result").value'),'人工校对的结果');
      await click('[data-mode="vision"]');await click('[data-action="recognize"]');await wait(2000);await screenshot('image-content-light');
    });
    await check('disable-plugin-blocks-recognition',async()=>{
      await click('[data-page="plugins"]');await screenshot('plugins-light');await click('[data-toggle="plugin:vision"]');await click('[data-page="image"]');assert.equal(await evaluate('document.querySelector("[data-action=recognize]").disabled'),true);
      await click('[data-page="plugins"]');await click('[data-toggle="plugin:vision"]');await click('[data-page="image"]');assert.equal(await evaluate('document.querySelector("[data-action=recognize]").disabled'),false);
    });
    await check('processing-is-inline-and-cancellable',async()=>{
      await click('[data-action="collage"]');assert.equal(await evaluate('document.querySelector("#overlay").hidden'),true);assert.ok(await evaluate('!!document.querySelector(".inspector #image-operation")'));await click('[data-action="close-panel"]');
      await click('[data-action="recognize"]');await click('[data-action="cancel-ocr"]');await wait(1900);assert.equal(await evaluate('S.jobs.at(-1).status'),'cancelled');
    });
    await check('record-follow-editor-and-export',async()=>{
      await click('[data-page="video"]');await screenshot('recording-light');await evaluate('document.querySelector("#record-fps").value="60 fps";document.querySelector("#record-fps").dispatchEvent(new Event("change",{bubbles:true}))');await click('[data-page="image"]');await click('[data-page="video"]');assert.equal(await evaluate('document.querySelector("#record-fps").value'),'60 fps');await click('[data-toggle="follow"]');assert.equal(await evaluate('S.record.follow'),false);
      await click('[data-action="start-record"]');await wait(1100);await click('[data-action="stop-record"]');assert.equal(await evaluate('S.record.editing'),true);
      await click('[data-toggle="follow"]');await screenshot('video-editor-light');await click('[data-action="export"]');assert.equal(await evaluate('document.querySelector("#overlay").hidden'),true);
      await evaluate('document.querySelector("#export-format").value="GIF";document.querySelector("#export-format").dispatchEvent(new Event("change",{bubbles:true}))');await screenshot('gif-export-light');await click('[data-action="close-panel"]');
      await click('[data-action="frame-capture"]');assert.equal(await evaluate('S.page'),'image');await click('[data-page="video"]');assert.equal(await evaluate('S.record.editing'),true);
    });
    await check('asset-filter-and-service-page',async()=>{
      await click('[data-page="library"]');await screenshot('library-light');await click('[data-filter="video"]');assert.equal(await evaluate('document.querySelectorAll(".asset-card").length'),1);
      await click('[data-page="settings"]');await screenshot('settings-light');await click('[data-settings="service"]');await screenshot('service-light');
      assert.equal(await evaluate('document.querySelector("#api-key").type'),'password');
    });
    await check('keyboard-command-and-theme',async()=>{
      await pageSocket.send('Input.dispatchKeyEvent',{type:'keyDown',key:'k',code:'KeyK',windowsVirtualKeyCode:75,modifiers:2});await wait(80);assert.equal(await evaluate('document.querySelector("#overlay").hidden'),false);
      await pageSocket.send('Input.dispatchKeyEvent',{type:'keyDown',key:'Escape',code:'Escape',windowsVirtualKeyCode:27});await wait(80);assert.equal(await evaluate('document.querySelector("#overlay").hidden'),true);
      await click('#theme-toggle');assert.equal(await evaluate('document.documentElement.dataset.theme'),'dark');await click('[data-page="image"]');await screenshot('image-content-dark');
    });
    await check('compact-viewports',async()=>{
      await click('#theme-toggle');
      for(const [width,height] of [[1170,820],[960,720]]){
        await pageSocket.send('Emulation.setDeviceMetricsOverride',{width,height,deviceScaleFactor:1,mobile:false});await wait(120);
        for(const page of ['image','video','library','plugins','settings']){
          await click(`[data-page="${page}"]`);
          const box=await evaluate('({w:innerWidth,sw:document.documentElement.scrollWidth,status:document.querySelector(".status-bar").getBoundingClientRect().bottom,main:document.querySelector(".main").getBoundingClientRect().right})');
          assert.ok(box.sw<=width,`${page}: viewport overflow ${box.sw}/${width}`);assert.ok(box.status<=height+1);assert.ok(box.main<=width+1);
          if(page==='image'){assert.ok(await evaluate('document.querySelector("#document-stage").getBoundingClientRect().width>220'));assert.ok(await evaluate('document.querySelector("#canvas").scrollHeight<=document.querySelector("#canvas").clientHeight+1'));}
        }
        await click('[data-page="image"]');await screenshot(`image-${width}x${height}`);
      }
    });
    assert.equal(errors.length,0,JSON.stringify(errors));
    const report={runId,status:'Pass',scope:'design-prototype-not-native-app',checks,javascriptErrors:errors};
    fs.writeFileSync(path.join(output,'prototype-tests.json'),JSON.stringify(report,null,2));
    const previews=path.join(root,'docs/prototypes/workspace-v2/previews');fs.mkdirSync(previews,{recursive:true});
    for(const file of fs.readdirSync(output).filter(f=>f.endsWith('.png')||f.endsWith('.json')))fs.copyFileSync(path.join(output,file),path.join(previews,file));
    console.log(`Prototype evidence: ${output}`);
  } catch (error) {
    fs.writeFileSync(path.join(output, 'prototype-tests.json'), JSON.stringify({ runId, status: 'Fail', checks, error: error.stack, javascriptErrors: errors }, null, 2));
    console.error(error); process.exitCode = 1;
  } finally {
    pageSocket?.socket.close();
    if (browserSocket) { await browserSocket.send('Browser.close').catch(() => {}); browserSocket.socket.close(); }
    else child.kill();
  }
})();

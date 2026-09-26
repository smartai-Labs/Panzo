// Tests only an isolated headless Chromium launched for this local prototype.
// Never connects to the user's browser/profile. No external packages or network assets.
const fs = require('node:fs');
const path = require('node:path');
const { spawn } = require('node:child_process');
const { pathToFileURL } = require('node:url');
const assert = require('node:assert/strict');
const root = path.resolve(__dirname, '..');
const runId = `r0-prototype-${new Date().toISOString().replace(/[:.]/g, '-')}`;
const output = path.join(root, 'docs', 'acceptance', 'RE-MVP', runId);
const profile = fs.mkdtempSync(path.join(root, '.tmp', 'r0-browser-'));
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
    await pageSocket.send('Page.navigate', { url: pathToFileURL(path.join(root, 'docs/prototypes/re-mvp/index.html')).href }); await wait(700);
    await check('editor-loaded', async () => assert.equal(await evaluate('document.querySelectorAll(".camera").length'), 1));
    await screenshot('editor-1440x900');
    await check('scrub-changes-time', async () => { const before = await evaluate('document.querySelector("#time-now").textContent'); await drag('#ruler', 220); assert.notEqual(await evaluate('document.querySelector("#time-now").textContent'), before); });
    await check('space-after-scrub-and-input-focus', async () => {
      await space(); assert.equal(await evaluate('document.querySelector("#play").getAttribute("aria-label")'), '暂停');
      await space(); assert.equal(await evaluate('document.querySelector("#play").getAttribute("aria-label")'), '播放');
      await click('#segment-scale'); await space(); assert.equal(await evaluate('document.querySelector("#play").getAttribute("aria-label")'), '播放');
    });
    await check('camera-trim-single-undo', async () => {
      const before = await evaluate('document.querySelector("#segment-end").value');
      await drag('.camera .handle.right', 50); const after = await evaluate('document.querySelector("#segment-end").value'); assert.notEqual(after, before);
      await click('#undo'); assert.equal(await evaluate('document.querySelector("#segment-end").value'), before);
    });
    await check('split-delete-undo', async () => {
      await click('#split'); assert.equal(await evaluate('document.querySelectorAll(".clip").length'), 2);
      await click('.clip'); await click('#delete'); assert.equal(await evaluate('document.querySelectorAll(".clip").length'), 1);
      await click('#undo'); assert.equal(await evaluate('document.querySelectorAll(".clip").length'), 2);
      await click('#undo'); assert.equal(await evaluate('document.querySelectorAll(".clip").length'), 1);
    });
    await check('command-button-not-latched', async () => { await click('#zoom-in'); assert.equal(await evaluate('document.querySelector("#zoom-in").matches(":active")'), false); assert.equal(await evaluate('document.querySelector("#zoom-in").getAttribute("aria-pressed")'), null); await click('#fit'); });
    await check('background-update', async () => { await click('[data-background="peach"]'); assert.equal(await evaluate('document.querySelector("#composition").classList.contains("peach")'), true); await click('[data-background="mist"]'); });
    await click('.camera');
    await check('compact-layout', async () => {
      await pageSocket.send('Emulation.setDeviceMetricsOverride', { width: 1024, height: 720, deviceScaleFactor: 1, mobile: false }); await wait(120);
      const layout = await evaluate(`(() => { const ids=['ruler','video-track','camera-track']; const boxes=ids.map(id=>{const r=document.getElementById(id).getBoundingClientRect(); return {top:r.top,bottom:r.bottom,right:r.right};}); return {width:innerWidth,scrollWidth:document.documentElement.scrollWidth,boxes,status:document.querySelector('.statusbar').getBoundingClientRect().bottom}; })()`);
      assert.ok(layout.scrollWidth <= layout.width); assert.ok(layout.status <= 721);
      assert.ok(layout.boxes[0].bottom <= layout.boxes[1].top && layout.boxes[1].bottom <= layout.boxes[2].top);
      await screenshot('editor-1024x720');
    });
    await pageSocket.send('Emulation.setDeviceMetricsOverride', { width: 1440, height: 900, deviceScaleFactor: 1, mobile: false });
    await check('recorder-and-simulated-roundtrip', async () => {
      await click('[data-view="recorder"]'); await screenshot('recorder-1440x900');
      await click('#record'); assert.equal(await evaluate('document.querySelector("#record").classList.contains("recording")'), true);
      await screenshot('recorder-recording'); await click('#record'); await wait(600);
      assert.equal(await evaluate('document.querySelector("#editor").hidden'), false);
    });
    await check('export-cancel-and-complete', async () => {
      await click('#export-open'); await screenshot('export-settings'); await click('#export-start'); await wait(400); await screenshot('export-progress');
      await click('#export-cancel'); assert.equal(await evaluate('document.querySelector("#export-dialog").open'), false);
      await click('#export-open'); await click('#export-start'); await wait(2900);
      assert.equal(await evaluate('document.querySelector("#export-progress-text").textContent'), '演示完成，没有生成视频文件');
      await click('#export-cancel');
    });
    if (process.env.PANZO_R0_VIDEO) {
      await check('real-local-video-preview', async () => {
        const document = await pageSocket.send('DOM.getDocument');
        const input = await pageSocket.send('DOM.querySelector', { nodeId: document.root.nodeId, selector: '#video-file' });
        await pageSocket.send('DOM.setFileInputFiles', { nodeId: input.nodeId, files: [path.resolve(process.env.PANZO_R0_VIDEO)] }); await wait(1500);
        assert.ok(await evaluate('document.querySelector("#source-video").videoWidth > 0'));
        await screenshot('editor-real-local-video');
      });
    }
    assert.equal(errors.length, 0, JSON.stringify(errors));
    fs.writeFileSync(path.join(output, 'prototype-tests.json'), JSON.stringify({ runId, status: 'Pass', scope: 'isolated-browser-prototype-not-native-app', checks, javascriptErrors: errors }, null, 2));
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

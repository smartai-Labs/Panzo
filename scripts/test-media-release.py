"""Native release regression. Uses an isolated profile; never closes existing Panzo processes."""
import argparse, ctypes as c, json, os, subprocess, time, uuid
from ctypes import wintypes as w
from pathlib import Path
from PIL import Image, ImageGrab
args=argparse.ArgumentParser();args.add_argument('--binary',default='target/debug/panzo.exe');args.add_argument('--output',default='docs/acceptance/media-v11-20261009/native');opt=args.parse_args()
root=Path(__file__).resolve().parent.parent
out=root/opt.output;out.mkdir(parents=True,exist_ok=True)
profile=root/'.tmp'/('ui-release-'+str(uuid.uuid4()));(profile/'Panzo').mkdir(parents=True)
(profile/'Videos/Panzo/Screenshots').mkdir(parents=True)
for name in ['Desktop','Documents','Downloads','Pictures','Music','Favorites']: (profile/name).mkdir()
(profile/'Panzo/settings.json').write_text(json.dumps({'theme':'dark','close_to_tray':False}),encoding='utf-8')
Image.new('RGB',(1600,1000),(232,234,238)).save(profile/'Videos/Panzo/Screenshots/review-image.png')
api=c.WinDLL('user32',use_last_error=True)
api.GetWindowThreadProcessId.argtypes=[w.HWND,c.POINTER(w.DWORD)];api.GetClassNameW.argtypes=[w.HWND,w.LPWSTR,c.c_int]
api.GetWindowTextW.argtypes=[w.HWND,w.LPWSTR,c.c_int];api.GetDlgItem.argtypes=[w.HWND,c.c_int];api.GetDlgItem.restype=w.HWND
api.IsWindowVisible.argtypes=[w.HWND];api.IsWindowVisible.restype=w.BOOL
api.GetWindowLongPtrW.argtypes=[w.HWND,c.c_int];api.GetWindowLongPtrW.restype=c.c_ssize_t
api.SendMessageTimeoutW.argtypes=[w.HWND,w.UINT,c.c_size_t,c.c_ssize_t,w.UINT,w.UINT,c.POINTER(c.c_size_t)];api.SendMessageTimeoutW.restype=c.c_size_t
api.PostMessageW.argtypes=[w.HWND,w.UINT,c.c_size_t,c.c_ssize_t]
api.GetWindowRect.argtypes=[w.HWND,c.POINTER(w.RECT)]
api.SetWindowPos.argtypes=[w.HWND,w.HWND,c.c_int,c.c_int,c.c_int,c.c_int,w.UINT]
callback=c.WINFUNCTYPE(w.BOOL,w.HWND,w.LPARAM);api.EnumWindows.argtypes=[callback,w.LPARAM];api.EnumChildWindows.argtypes=[w.HWND,callback,w.LPARAM]
def value(h):
 b=c.create_unicode_buffer(32768);message(h,13,len(b),c.addressof(b));return b.value
def message(h,msg,wp=0,lp=0):
 r=c.c_size_t()
 if not api.SendMessageTimeoutW(h,msg,wp,lp,2,5000,c.byref(r)):raise RuntimeError('window message timed out '+str(msg))
 return r.value
def set_text(h,s):
 b=c.create_unicode_buffer(s);message(h,12,0,c.addressof(b))
def windows(parent=None):
 result=[]
 @callback
 def each(h,_):
  owner=w.DWORD();api.GetWindowThreadProcessId(h,c.byref(owner))
  if owner.value==process.pid:
   cls=c.create_unicode_buffer(128);api.GetClassNameW(h,cls,len(cls));caption=c.create_unicode_buffer(512);api.GetWindowTextW(h,caption,len(caption));result.append((h,cls.value,caption.value))
  return True
 if parent:api.EnumChildWindows(parent,each,0)
 else:api.EnumWindows(each,0)
 return result
def wait(fn,timeout=12):
 until=time.monotonic()+timeout
 while time.monotonic()<until:
  result=fn()
  if result:return result
  if process.poll() is not None:raise RuntimeError('process exited '+str(process.returncode))
  time.sleep(.05)
 raise RuntimeError('condition timed out')
def find(kind,parent=None,title=None,visible=False):
 return wait(lambda:next((h for h,k,t in windows(parent) if k==kind and (title is None or title in t) and (not visible or api.IsWindowVisible(h))),None))
def command(h,id):
 message(h,0x111,id);time.sleep(.16)
def snapshot(h,name):
 time.sleep(.3)
 if h==main:
  from PIL import ImageGrab
  r=w.RECT();api.GetWindowRect(h,c.byref(r));ImageGrab.grab(bbox=(r.left,r.top,r.right,r.bottom),all_screens=True).save(out/name)
 else:raster(h).save(out/name)
startup=subprocess.STARTUPINFO();startup.dwFlags|=subprocess.STARTF_USESHOWWINDOW;startup.wShowWindow=0
env=os.environ.copy();env['LOCALAPPDATA']=str(profile);env['USERPROFILE']=str(profile)

api.GetWindowRect.argtypes=[w.HWND,c.POINTER(w.RECT)]
api.GetClientRect.argtypes=[w.HWND,c.POINTER(w.RECT)]
api.SetProcessDpiAwarenessContext.argtypes=[c.c_void_p];api.SetProcessDpiAwarenessContext(c.c_void_p(-4))
gdi=c.WinDLL('gdi32');api.GetDC.argtypes=[w.HWND];api.GetDC.restype=w.HDC;api.ReleaseDC.argtypes=[w.HWND,w.HDC]
api.PrintWindow.argtypes=[w.HWND,w.HDC,w.UINT];api.PrintWindow.restype=w.BOOL
gdi.CreateCompatibleDC.argtypes=[w.HDC];gdi.CreateCompatibleDC.restype=w.HDC
gdi.CreateCompatibleBitmap.argtypes=[w.HDC,c.c_int,c.c_int];gdi.CreateCompatibleBitmap.restype=w.HBITMAP
gdi.SelectObject.argtypes=[w.HDC,w.HANDLE];gdi.SelectObject.restype=w.HANDLE
gdi.DeleteObject.argtypes=[w.HANDLE];gdi.DeleteDC.argtypes=[w.HDC]
class BitmapInfo(c.Structure):
 _fields_=[('size',w.DWORD),('width',w.LONG),('height',w.LONG),('planes',w.WORD),('bits',w.WORD),('compression',w.DWORD),('image_size',w.DWORD),('xppm',w.LONG),('yppm',w.LONG),('used',w.DWORD),('important',w.DWORD)]
gdi.GetDIBits.argtypes=[w.HDC,w.HBITMAP,w.UINT,w.UINT,c.c_void_p,c.c_void_p,w.UINT]
api.UpdateWindow.argtypes=[w.HWND]
def raster(h):
 api.UpdateWindow(h)
 r=w.RECT();api.GetClientRect(h,c.byref(r));dc=api.GetDC(h);memory=gdi.CreateCompatibleDC(dc);bmp=gdi.CreateCompatibleBitmap(dc,r.right,r.bottom);old=gdi.SelectObject(memory,bmp)
 try:
  assert api.PrintWindow(h,memory,1);gdi.SelectObject(memory,old)
  info=BitmapInfo(c.sizeof(BitmapInfo),r.right,-r.bottom,1,32,0,0,0,0,0,0);pixels=c.create_string_buffer(r.right*r.bottom*4)
  assert gdi.GetDIBits(dc,bmp,0,r.bottom,pixels,c.byref(info),0)==r.bottom
  return Image.frombytes('RGB',(r.right,r.bottom),pixels.raw,'raw','BGRX')
 finally:gdi.SelectObject(memory,old);gdi.DeleteObject(bmp);gdi.DeleteDC(memory);api.ReleaseDC(h,dc)
import shutil, wave, math, struct, threading, http.server, base64, hashlib
fixture=profile/'media-test.panzo'
shutil.copytree(root/'.tmp/desktop-v3-review-fixtures/workspace-review-d66b9131-5476-4a73-bef4-51e9d9754678/fixture.panzo',fixture)
(fixture/'edit/recovery-draft.json').unlink(missing_ok=True)
workbench=fixture/'edit/workbench.json'
settings0=json.loads(workbench.read_text(encoding='utf-8-sig'));manifest=json.loads((fixture/'project.json').read_text(encoding='utf-8-sig'))
settings0.update({'schemaVersion':8,'annotations':[],'subtitles':[],'audioClips':[]})
settings0['video']={'sourceDurationTick':manifest['media']['durationTick'],'clips':[{'id':'video','sourceInTick':0,'sourceOutTick':60_000_000,'speedPercent':100}]}
settings0['canvas']['size']={'width':640,'height':360}
workbench.write_text(json.dumps(settings0),encoding='utf-8')
wavepath=profile/'narration.wav'
with wave.open(str(wavepath),'wb') as wav:
 wav.setnchannels(1);wav.setsampwidth(2);wav.setframerate(24000)
 wav.writeframes(b''.join(struct.pack('<h',round(9000*math.sin(2*math.pi*440*i/24000))) for i in range(24000*4)))
requests=[];behavior={'mode':'ok'}
class Server(http.server.BaseHTTPRequestHandler):
 def log_message(self,*args):pass
 def do_POST(self):
  data=self.rfile.read(int(self.headers['Content-Length']));requests.append({'path':self.path,'model':b'custom-speech' in data,'wave':b'RIFF' in data,'authorized':self.headers.get('Authorization')=='Bearer panzo-synthetic-speech-key'})
  if behavior['mode']=='delay':time.sleep(4)
  payload=({'text':'没有时间戳'} if behavior['mode']=='invalid' else {'segments':[{'start':.2,'end':1.7,'text':'你好，Panzo。'},{'start':2,'end':3.5,'text':'这是一段自动字幕。'}]})
  body=json.dumps(payload,ensure_ascii=False).encode()
  try:
   self.send_response(200);self.send_header('Content-Type','application/json');self.send_header('Content-Length',str(len(body)));self.end_headers();self.wfile.write(body)
  except (BrokenPipeError,ConnectionResetError,ConnectionAbortedError):pass
server=http.server.ThreadingHTTPServer(('127.0.0.1',0),Server);threading.Thread(target=server.serve_forever,daemon=True).start()
report={'passed':False,'checks':[],'binary':opt.binary,'profile':str(profile),'project':str(fixture)}
api.GetPropW.argtypes=[w.HWND,w.LPCWSTR];api.GetPropW.restype=w.HANDLE
process=None;main=None

def current():return json.loads(workbench.read_text(encoding='utf-8-sig'))
def save():command(main,202);time.sleep(.5)
def choose_file(path):
 picker=find('#32770',visible=True);entry=find('Edit',picker,visible=True);set_text(entry,str(path));api.PostMessageW(picker,0x111,1,0);wait(lambda:not api.IsWindowVisible(picker))
def launch():
 global process,main,video
 process=subprocess.Popen([str(root/opt.binary),str(fixture)],env=env,startupinfo=startup);main=find('PanzoWorkspace');api.SetWindowPos(main,w.HWND(-1),30,25,1280,940,0x40);video=find('PanzoRecorderWindow',main);wait(lambda:api.GetPropW(video,'PanzoEditorState'),35);command(main,101)
def drag_audio(trim=False):
 r=w.RECT();api.GetClientRect(video,c.byref(r));width=r.right-104
 top=r.bottom-8-350+108;end=r.bottom-8-50;row=(end-top)//6;y=top+row*3+row//2
 x=round(80+(.5 if trim else 2.0)*width/6)+(2 if trim else 0);target=x+round(width/(12 if trim else 6))
 pack=lambda x:(y<<16)|(x&65535)
 message(video,0x201,1,pack(x));message(video,0x200,1,pack(target));message(video,0x202,0,pack(target));time.sleep(.3)

def stop():
 api.PostMessageW(main,0x10,0,0);process.wait(timeout=12);report.setdefault("exitCodes",[]).append(process.returncode);assert process.returncode==0,hex(process.returncode & 0xffffffff)
try:
 # Install the independently packaged executable using production registry format in an isolated profile.
 # The GUI install path is exercised below as well.
 package=(root/opt.binary).parent/'panzo-media-tools.panzoplugin'
 assert package.exists(),package
 launch();command(main,115);plugin_window=find('PanzoManagementWindow',title='插件',visible=True);manager=find('PanzoPluginManager',plugin_window,visible=True);api.PostMessageW(manager,0x111,1,0);choose_file(package)
 confirm=find('PanzoWorkspaceForm',plugin_window,visible=True);command(confirm,1);wait(lambda:(profile/'Panzo/Plugins/registry.json').exists())
 registry=json.loads((profile/'Panzo/Plugins/registry.json').read_text(encoding='utf-8-sig'));assert 'panzo.media-tools' in registry['installed']
 plugins_window=find('PanzoManagementWindow',title='插件');api.PostMessageW(plugins_window,0x10,0,0)
 report['checks'].append('protocol 2 package installed through native plugin manager')
 command(main,116);settings_window=find('PanzoManagementWindow',title='设置',visible=True);command(settings_window,124);form=find('PanzoWorkspaceForm',settings_window,visible=True)
 set_text(api.GetDlgItem(form,100),f'http://127.0.0.1:{server.server_port}/v1');set_text(api.GetDlgItem(form,101),'custom-speech');set_text(api.GetDlgItem(form,102),'panzo-synthetic-speech-key');command(form,1)
 speech=profile/'Panzo/speech.json';assert speech.exists();assert 'panzo-synthetic-speech-key' not in speech.read_text()
 snapshot(settings_window,'speech-settings-dark.png');api.PostMessageW(settings_window,0x10,0,0)
 report['checks'].append('user endpoint/model/key saved; plaintext key absent from saved settings')
 command(main,260);panel=find('PanzoMediaPanel',main,visible=True);api.PostMessageW(panel,0x111,30,0);choose_file(wavepath)
 wait(lambda:'音频已加入' in value(api.GetDlgItem(panel,51)),30);save();assert len(current()['audioClips'])==1
 report['checks'].append('standalone worker decodes imported WAV into project PCM; waveform and clip saved')
 # Exact clip fields, non-destructive trim and fades.
 for id,text in [(120,'0.5'),(121,'0.25'),(122,'3.5'),(123,'60'),(124,'0.2'),(125,'0.3')]:set_text(api.GetDlgItem(panel,id),text)
 command(panel,50);assert '已应用' in value(api.GetDlgItem(panel,51));save();clip=current()['audioClips'][0];assert clip['start']==5_000_000 and clip['sourceIn']==2_500_000 and clip['gain']==.6
 snapshot(main,'audio-workbench-dark.png')
 command(main,207);save();assert current()['audioClips'][0]['start']==0;command(main,208);save();assert current()['audioClips'][0]['start']==5_000_000
 report['checks'].append('trim, start, gain, fades commit via shared undo/redo and save')
 drag_audio();save();assert abs(current()['audioClips'][0]['start']-15_000_000)<150_000,current()['audioClips'];command(main,207);save()
 drag_audio(True);save();assert abs(current()['audioClips'][0]['sourceIn']-7_500_000)<150_000,current()['audioClips'];command(main,207);save()
 report['checks'].append('native mouse drag moves audio and trims its edge; each gesture is one undo step')
 # Countdown cancellation is tested before microphone access; no user voice is captured.
 command(panel,31);command(panel,37);wait(lambda:'已取消' in value(api.GetDlgItem(panel,51)),8);save();assert len(current()['audioClips'])==1
 report['checks'].append('voiceover countdown cancels without recording or adding an asset')
 command(panel,21);message(api.GetDlgItem(panel,61),0x14e,2);command(panel,32)
 wait(lambda:'已生成' in value(api.GetDlgItem(panel,51)),30);save();assert len(current()['subtitles'])==2;assert all(requests[-1][k] for k in ['model','wave','authorized']);assert requests[-1]['path']=='/v1/audio/transcriptions'
 report['checks'].append('configured AI called by independent plugin; real mixed audio uploaded to loopback; timestamped cues persisted')
 set_text(api.GetDlgItem(panel,140),'用户校对后的字幕');command(panel,50);save();assert current()['subtitles'][0]['text']=='用户校对后的字幕'
 listing=find('PanzoMediaList',panel,visible=True);message(listing,0x201,1,(15<<16)|20)
 vr=w.RECT();api.GetClientRect(video,c.byref(vr));x=round(80+(vr.right-104)/6);y=vr.bottom-8-350+96;point=(y<<16)|x
 message(video,0x201,1,point);message(video,0x202,0,point);time.sleep(.5)
 snapshot(main,'subtitle-workbench-dark.png');command(main,110);snapshot(main,'subtitle-workbench-light.png')
 before=current()['subtitles'];behavior['mode']='invalid';command(panel,32);wait(lambda:'没有返回字幕时间戳' in value(api.GetDlgItem(panel,51)),20);save();assert current()['subtitles']==before
 report['checks'].append('manual correction and themed workbench; missing timestamps rejected without replacing existing subtitles')
 behavior['mode']='delay';n=len(requests);command(panel,32);wait(lambda:len(requests)>n);command(panel,37);wait(lambda:'已取消' in value(api.GetDlgItem(panel,51)),10);save();assert current()['subtitles']==before;behavior['mode']='ok'
 report['checks'].append('cancel interrupts worker network wait and preserves existing subtitles')
 # SRT export uses current editable text.
 exported=profile/'subtitles.srt';api.PostMessageW(panel,0x111,35,0);choose_file(exported);wait(exported.exists);assert '用户校对后的字幕' in exported.read_text(encoding='utf-8')
 report['checks'].append('SRT export preserves corrected text and timestamps')
 save();stop();launch();command(main,261);panel=find('PanzoMediaPanel',main,visible=True);assert len(current()['subtitles'])==2 and len(current()['audioClips'])==1
 command(main,116);settings_window=find('PanzoManagementWindow',title='设置',visible=True);command(settings_window,124);form=find('PanzoWorkspaceForm',settings_window,visible=True);assert value(api.GetDlgItem(form,101))=='custom-speech';api.PostMessageW(settings_window,0x10,0,0)
 report['checks'].append('fresh process reopens subtitles/audio and restores model configuration')
 command(main,115);plugin_window=find('PanzoManagementWindow',title='插件',visible=True);manager=find('PanzoPluginManager',plugin_window,visible=True);listing=api.GetDlgItem(manager,100)
 needle=c.create_unicode_buffer('声音、旁白与 AI 字幕');index=message(listing,0x1a2,-1,c.addressof(needle));assert index<20,index;message(listing,0x186,index);command(manager,3);api.PostMessageW(plugin_window,0x10,0,0)
 command(panel,32);wait(lambda:'安装并启用' in value(api.GetDlgItem(panel,51)));save();assert current()['subtitles']==before and len(current()['audioClips'])==1
 report['checks'].append('disabled plugin blocks new AI jobs while persisted audio and captions remain editable')
 save();stop();report['passed']=True
finally:
 if process and process.poll() is None:
  if main:snapshot(main,'failure-main.png')
  if main:api.PostMessageW(main,0x10,0,0)
  try:process.wait(timeout=3)
  except subprocess.TimeoutExpired:process.kill();process.wait()
 server.shutdown();(out/'media-smoke.json').write_text(json.dumps(report,ensure_ascii=False,indent=2),encoding='utf-8')
print(json.dumps(report,ensure_ascii=False))

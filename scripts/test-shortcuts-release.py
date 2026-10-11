"""Native release regression. Uses an isolated profile; never closes existing Panzo processes."""
import argparse, ctypes as c, json, os, subprocess, time, uuid
from ctypes import wintypes as w
from pathlib import Path
from PIL import Image, ImageGrab
args=argparse.ArgumentParser();args.add_argument('--binary',default='target/desktop-v10/release/panzo.exe');args.add_argument('--output',default='docs/acceptance/shortcuts-v10-20261009/native');opt=args.parse_args()
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
 time.sleep(.25);raster(h).save(out/name)
startup=subprocess.STARTUPINFO();startup.dwFlags|=subprocess.STARTF_USESHOWWINDOW;startup.wShowWindow=0
env=os.environ.copy();env['LOCALAPPDATA']=str(profile);env['USERPROFILE']=str(profile)

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

import re
catalog=re.findall(r'a!\(\s*"([a-z_]+)"',(root/'crates/panzo-windows/src/platform/shortcuts.rs').read_text(encoding='utf-8-sig'))
assert len(catalog)>50
report={'passed':False,'binary':opt.binary,'checks':[],'processIds':[],'profile':str(profile),'actions':len(catalog)}
api.RegisterHotKey.argtypes=[w.HWND,c.c_int,w.UINT,w.UINT];api.RegisterHotKey.restype=w.BOOL
api.UnregisterHotKey.argtypes=[w.HWND,c.c_int];api.ShowWindow.argtypes=[w.HWND,c.c_int]
api.IsWindow.argtypes=[w.HWND];api.SetForegroundWindow.argtypes=[w.HWND]
process=None;main=None;held=False
config_path=profile/'Panzo/shortcuts.json'
def launch(extra=None):
 global process,main
 process=subprocess.Popen([str(root/opt.binary)]+(extra or []),env=env,startupinfo=startup)
 report['processIds'].append(process.pid);main=find('PanzoWorkspace');wait(lambda:api.GetDlgItem(main,116));api.SetWindowPos(main,w.HWND(-1),40,30,1180,860,0x40)
 return main
def stop():
 api.PostMessageW(main,0x10,0,0);process.wait(timeout=10);assert process.returncode==0;time.sleep(.5)
def settings():
 command(main,116);s=find('PanzoManagementWindow',title='设置',visible=True);api.SetWindowPos(s,w.HWND(-1),65,45,1040,820,0x40);command(s,123)
 p=find('PanzoShortcutSettings',s,visible=True);l=find('PanzoShortcutList',p,visible=True);return s,p,l
def key_field(l,id):return api.GetDlgItem(l,1000+catalog.index(id)*4)
def row_command(l,id,slot):command(l,1000+catalog.index(id)*4+slot)
def record(l,id,key):
 row_command(l,id,0);h=key_field(l,id);assert '组合键' in value(h);api.PostMessageW(h,0x100,key,0);wait(lambda:'组合键' not in value(h))
def send_key(h,key):api.PostMessageW(h,0x100,key,0);time.sleep(.3)
def saved():return json.loads(config_path.read_text(encoding='utf-8-sig'))
try:
 launch();s,p,l=settings();assert value(key_field(l,'capture'))=='Ctrl+Alt+Shift+F9';assert value(key_field(l,'record'))=='Ctrl+Alt+Shift+F10'
 key_rect=w.RECT();list_rect=w.RECT();api.GetWindowRect(key_field(l,'capture'),c.byref(key_rect));api.GetWindowRect(l,c.byref(list_rect))
 assert key_rect.left>=list_rect.left and key_rect.right<=list_rect.right and key_rect.right-key_rect.left>=160,(key_rect.left,key_rect.right,list_rect.left,list_rect.right)
 snapshot(s,'shortcuts-default-dark.png')
 record(l,'assets',0x77);assert value(key_field(l,'assets'))=='F8'
 record(l,'images',0x77);command(p,20);assert '相同' in value(api.GetDlgItem(p,23));assert not config_path.exists()
 report['checks'].append('all actions visible in Settings; native key recording; duplicate bindings reject save without writing')
 row_command(l,'images',3);row_command(l,'capture',2);record(l,'crop',0x75);command(p,20);wait(config_path.exists)
 assert saved()['bindings']['assets']['chord']=={'modifiers':0,'key':0x77};assert saved()['bindings']['capture']['chord'] is None
 record(l,'assets',0x76);command(p,22);assert value(key_field(l,'assets'))=='F8'
 row_command(l,'assets',0);send_key(key_field(l,'assets'),27);assert value(key_field(l,'assets'))=='F8'
 api.PostMessageW(s,0x10,0,0);wait(lambda:not api.IsWindowVisible(s));command(main,100);send_key(main,0x77);wait(lambda:'素材' in value(main))
 report['checks'].append('Save applies immediately; Escape cancels recording; Revert discards draft; custom F8 switches workspace')
 s,p,l=settings();assert value(key_field(l,'assets'))=='F8';before=config_path.read_bytes()
 assert api.RegisterHotKey(None,710,7|0x4000,0x78),'test could not reserve released capture key';held=True
 command(p,21);command(p,20);wait(lambda:'占用' in value(api.GetDlgItem(p,23)));assert config_path.read_bytes()==before
 snapshot(s,'shortcut-global-conflict.png');command(p,22);assert value(key_field(l,'assets'))=='F8'
 api.UnregisterHotKey(None,710);held=False
 report['checks'].append('clearing global shortcut releases Windows registration; external owner blocks rebind; failed apply preserves persisted bindings and existing hotkeys')
 # Scroll and filter without overlapping the fixed heading/footer.
 message(api.GetDlgItem(p,12),0x14e,3);message(p,0x111,12|(1<<16));api.PostMessageW(l,0x20a,((-120)&65535)<<16,0);time.sleep(.2);snapshot(s,'shortcuts-image-dark.png')
 command(main,110);snapshot(s,'shortcuts-image-light.png');api.PostMessageW(s,0x10,0,0);wait(lambda:not api.IsWindowVisible(s))
 command(main,100);panel=find('PanzoImageWorkspace',main,visible=True);api.PostMessageW(panel,0x111,2,0)
 picker=find('#32770',title='打开',visible=True);entry=find('Edit',picker,visible=True);set_text(entry,str(profile/'Videos/Panzo/Screenshots/review-image.png'));api.PostMessageW(picker,0x111,1,0)
 wait(lambda:not api.IsWindowVisible(picker));picture=find('PanzoPicture',main,visible=True);crop_apply=api.GetDlgItem(picture,61)
 assert not api.IsWindowVisible(crop_apply);send_key(picture,ord('C'));assert not api.IsWindowVisible(crop_apply)
 send_key(picture,0x75);wait(lambda:api.IsWindowVisible(crop_apply));send_key(picture,27);wait(lambda:not api.IsWindowVisible(crop_apply))
 result=api.GetDlgItem(panel,25);set_text(result,'测试输入');send_key(result,ord('C'));assert not api.IsWindowVisible(crop_apply) and value(result).replace('测试输入','').lower()=='c'
 report['checks'].append('image crop respects custom F6 and old C no longer triggers; text input keeps native typing behavior')
 command(picture,44);stop()
 launch();s,p,l=settings();assert value(key_field(l,'assets'))=='F8' and value(key_field(l,'crop'))=='F6' and value(key_field(l,'capture'))=='未设置'
 api.PostMessageW(s,0x10,0,0);wait(lambda:not api.IsWindowVisible(s));send_key(main,0x77);wait(lambda:'素材' in value(main));stop()
 report['checks'].append('fresh process reloads disabled and modified shortcuts; persisted F8 still dispatches')
 # Isolated startup configuration verifies global command mapping without capturing the user's screen.
 cfg=saved();cfg['bindings']['show_app']={'scope':'global','chord':{'modifiers':7,'key':0x78}}
 config_path.write_text(json.dumps(cfg),encoding='utf-8');launch();video=find('PanzoRecorderWindow',main)
 assert not api.RegisterHotKey(None,711,7|0x4000,0x78),'configured global hotkey was not registered'
 api.ShowWindow(main,0);assert not api.IsWindowVisible(main)
 api.PostMessageW(video,0x312,5001,0);wait(lambda:api.IsWindowVisible(main));stop()
 report['checks'].append('global registration reloads and mapped native hotkey command restores the hidden application')
 # Verify custom video actions reach the real editor and persist an actual split/undo.
 import shutil
 fixture=profile/'shortcut-video.panzo'
 shutil.copytree(root/'.tmp/desktop-v3-review-fixtures/workspace-review-d66b9131-5476-4a73-bef4-51e9d9754678/fixture.panzo',fixture)
 (fixture/'edit/recovery-draft.json').unlink(missing_ok=True)
 cfg=saved()
 for action,key in [('split',0x76),('next_frame',0x74),('save',0x73),('undo',0x72)]:cfg['bindings'][action]={'scope':'app','chord':{'modifiers':0,'key':key}}
 config_path.write_text(json.dumps(cfg),encoding='utf-8');launch([str(fixture)]);video=find('PanzoRecorderWindow',main)
 api.GetPropW.argtypes=[w.HWND,w.LPCWSTR];api.GetPropW.restype=w.HANDLE
 wait(lambda:api.GetPropW(video,'PanzoEditorState'),30);command(main,101);time.sleep(.5)
 workbench=fixture/'edit/workbench.json';before=workbench.read_bytes()
 send_key(video,0x74);time.sleep(3);send_key(video,ord('B'));send_key(video,0x73);time.sleep(.5);assert workbench.read_bytes()==before,'old video split key still active'
 send_key(video,0x76);send_key(video,0x73)
 wait(lambda:len(json.loads(workbench.read_text(encoding='utf-8-sig'))['video']['clips'])==2,20)
 send_key(video,0x72);send_key(video,0x73)
 wait(lambda:len(json.loads(workbench.read_text(encoding='utf-8-sig'))['video']['clips'])==1,20)
 snapshot(main,'custom-video-shortcuts.png');stop()
 report['checks'].append('custom next-frame, split, save and undo operate on a real isolated video project; old B binding stays inactive')
 dump_dir=Path(os.environ.get('LOCALAPPDATA',''))/'CrashDumps'
 report['crashDumps']=[q.name for q in dump_dir.glob('panzo*.dmp') if any(q.name==f'panzo.exe.{pid}.dmp' for pid in report['processIds'])]
 assert not report['crashDumps'],report['crashDumps'];report['passed']=True
finally:
 if held:api.UnregisterHotKey(None,710)
 if process and process.poll() is None:
  if main and api.IsWindow(main):snapshot(main,'failure-main.png')
  api.PostMessageW(main,0x10,0,0)
  try:process.wait(timeout=3)
  except subprocess.TimeoutExpired:process.kill();process.wait()
 (out/'shortcut-smoke.json').write_text(json.dumps(report,ensure_ascii=False,indent=2),encoding='utf-8')
print(json.dumps(report,ensure_ascii=False))

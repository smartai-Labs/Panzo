"""Native release regression. Uses an isolated profile; never closes existing Panzo processes."""
import argparse, ctypes as c, json, os, subprocess, time, uuid
from ctypes import wintypes as w
from pathlib import Path
from PIL import Image, ImageGrab
args=argparse.ArgumentParser();args.add_argument('--baseline',action='store_true');args.add_argument('--binary',default='target/desktop-v8/release/panzo.exe');args.add_argument('--output',default='docs/acceptance/exit-dialogs-v8-20261009/native');opt=args.parse_args()
root=Path(__file__).resolve().parent.parent
out=root/opt.output;out.mkdir(parents=True,exist_ok=True)
profile=root/'.tmp'/('exit-review-'+str(uuid.uuid4()));(profile/'Panzo').mkdir(parents=True)
(profile/'Videos/Panzo/Screenshots').mkdir(parents=True)
for name in ['Desktop','Documents','Downloads','Pictures','Music','Favorites']: (profile/name).mkdir()
(profile/'Panzo/settings.json').write_text(json.dumps({'theme':'dark','close_to_tray':False}),encoding='utf-8')
Image.new('RGB',(1600,1000),(232,234,238)).save(profile/'Videos/Panzo/Screenshots/review-image.png')
api=c.WinDLL('user32',use_last_error=True)
api.SetProcessDpiAwarenessContext.argtypes=[c.c_void_p];api.SetProcessDpiAwarenessContext(c.c_void_p(-4))
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
 time.sleep(.25);r=w.RECT();api.GetWindowRect(h,c.byref(r));ImageGrab.grab(bbox=(r.left,r.top,r.right,r.bottom)).save(out/name)

api.SetForegroundWindow.argtypes=[w.HWND];api.SetCursorPos.argtypes=[c.c_int,c.c_int];api.GetClientRect.argtypes=[w.HWND,c.POINTER(w.RECT)];api.ClientToScreen.argtypes=[w.HWND,c.POINTER(w.POINT)]
api.GetComboBoxInfo.argtypes=[w.HWND,c.c_void_p]
class ComboInfo(c.Structure):_fields_=[('size',w.DWORD),('item',w.RECT),('button',w.RECT),('state',w.DWORD),('combo',w.HWND),('edit',w.HWND),('list',w.HWND)]
def combo_info(h):
 info=ComboInfo();info.size=c.sizeof(info);assert api.GetComboBoxInfo(h,c.byref(info));return info

def bounds(h):
 r=w.RECT();api.GetWindowRect(h,c.byref(r));return r

def click(h,x,y):
 r=bounds(h);api.SetCursorPos(r.left+x,r.top+y);api.PostMessageW(h,0x201,1,(y<<16)|x);api.PostMessageW(h,0x202,0,(y<<16)|x)

def choose(h,index):
 message(h,0x14e,index);parent=api.GetParent(h);message(parent,0x111,api.GetDlgCtrlID(h)|(1<<16),h);time.sleep(.08)
api.GetParent.argtypes=[w.HWND];api.GetParent.restype=w.HWND;api.GetDlgCtrlID.argtypes=[w.HWND];api.IsWindowEnabled.argtypes=[w.HWND]
import hashlib
api.ShowWindow.argtypes=[w.HWND,c.c_int]
api.IsWindow.argtypes=[w.HWND];api.IsWindow.restype=w.BOOL
report={'binary':opt.binary,'binarySha256':hashlib.sha256((root/opt.binary).read_bytes()).hexdigest(),'baseline':opt.baseline,'passed':False,'checks':[],'processIds':[]}
dump_dir=Path(os.environ['LOCALAPPDATA'])/'CrashDumps'
dumps_before={p.name:p.stat().st_mtime_ns for p in dump_dir.glob('panzo*.dmp')}
process=None;main=None
screenshots=profile/'Videos/Panzo/Screenshots'

def dialogs():return [(h,t) for h,k,t in windows() if k==('\x2332770' if opt.baseline else 'PanzoMessageDialog') and api.IsWindowVisible(h)]
def picture_windows():return [h for h,k,t in windows(main) if k=='PanzoPicture']
def dialog_text(h):return '\n'.join(t for _,k,t in windows(h) if k in ('Static','Edit'))
def observe_one(expected,duration=1.3):
 until=time.monotonic()+duration;maximum=0
 while time.monotonic()<until:
  present=dialogs();maximum=max(maximum,len(present));assert len(present)==1 and present[0][0]==expected,{'dialogs':present,'exitCode':process.poll(),'pid':process.pid}
  time.sleep(.025)
 return maximum

def launch():
 global process,main,video,panel
 time.sleep(.5)
 startup=subprocess.STARTUPINFO();startup.dwFlags|=subprocess.STARTF_USESHOWWINDOW;startup.wShowWindow=0
 env=os.environ.copy();env['LOCALAPPDATA']=str(profile);env['USERPROFILE']=str(profile)
 (profile/'Panzo/settings.json').write_text(json.dumps({'theme':'dark','close_to_tray':True}),encoding='utf-8')
 process=subprocess.Popen([str(root/opt.binary)],env=env,startupinfo=startup)
 report['processIds'].append(process.pid)
 main=find('PanzoWorkspace');wait(lambda:api.GetDlgItem(main,116));api.SetWindowPos(main,w.HWND(-1),30,30,1100,820,0x40)
 video=find('PanzoRecorderWindow',main);panel=find('PanzoImageWorkspace',main,visible=True)

def load_dirty():
 files=[]
 for i in range(2):
  path=profile/f'original-{i}.png';Image.new('RGB',(320+i*20,160+i*20),[(180,80,40),(40,120,180)][i]).save(path);files.append(path)
 before={str(p):hashlib.sha256(p.read_bytes()).hexdigest() for p in files}
 api.PostMessageW(panel,0x111,2,0);picker=find('#32770',title='打开',visible=True);entry=find('Edit',picker,visible=True);set_text(entry,' '.join('"'+str(p)+'"' for p in files));api.PostMessageW(picker,0x111,1,0)
 wait(lambda:not api.IsWindowVisible(picker));wait(lambda:len(picture_windows())==2)
 for h in picture_windows():command(h,40)
 return before

def exit_request():api.PostMessageW(video,0x312,12,0)
def burst():
 for i in range(80):
  api.PostMessageW(video,0x801f,0,0);api.PostMessageW(video,0x312,12,0);api.PostMessageW(main,0x10,0,0)
  for h in picture_windows():api.PostMessageW(h,0x10,0,0)
def answer(h,id):api.PostMessageW(h,0x111,id,0)
def assert_sources(before):
 for path,digest in before.items():assert hashlib.sha256(Path(path).read_bytes()).hexdigest()==digest

def finish():
 process.wait(timeout=12);assert process.returncode==0
try:
 launch();originals=load_dirty()
 api.PostMessageW(main,0x10,0,0);wait(lambda:not api.IsWindowVisible(main));exit_request()
 if opt.baseline:
  wait(lambda:len(dialogs())>=2,8)
  report['reproducedDialogs']=[t for h,t in dialogs()]
  report['checks'].append('v7 single tray exit creates multiple simultaneous Close Image dialogs with no further input')
  report['passed']=True
 else:
  confirm=find('PanzoMessageDialog',title='退出 Panzo',visible=True);assert '2 张图片' in dialog_text(confirm)
  assert not (api.GetWindowLongPtrW(confirm,-20)&1),'confirmation must not use classic modal frame'
  burst();observe_one(confirm);snapshot(confirm,'one-exit-confirmation.png');answer(confirm,2)
  wait(lambda:not dialogs());time.sleep(1.3);assert process.poll() is None and len(picture_windows())==2 and not dialogs()
  assert_sources(originals);report['checks'].append('single combined two-image confirmation survives 80 repeated exit/close requests; Cancel preserves both pictures and stops automatic retries')
  # Independently closing one document shares the same modal guard.
  first=picture_windows()[0];api.PostMessageW(first,0x10,0,0);confirm=find('PanzoMessageDialog',title='关闭图片',visible=True)
  burst();observe_one(confirm);answer(confirm,2);wait(lambda:not dialogs());time.sleep(.5);assert len(picture_windows())==2 and process.poll() is None
  report['checks'].append('individual picture confirmation cannot reenter or start nested application exits')
  exit_request();confirm=find('PanzoMessageDialog',title='退出 Panzo',visible=True);answer(confirm,7);finish()
  assert_sources(originals);assert not list(screenshots.glob('截图-*.png'));report['checks'].append('Discard exits once, writes no screenshots and leaves source files untouched')
  launch();originals=load_dirty();exit_request();confirm=find('PanzoMessageDialog',title='退出 Panzo',visible=True);answer(confirm,6);finish()
  saved=list(screenshots.glob('截图-*.png'));assert len(saved)==2;assert sorted(Image.open(p).size for p in saved)==[(160,320),(180,340)];assert_sources(originals)
  report['checks'].append('Save All writes both edited images with rotated dimensions and exits successfully')
  # A private fixture file occupying Screenshots makes saving fail without changing permissions.
  launch();originals=load_dirty();backup=screenshots.with_name('Screenshots-review-backup')
  assert screenshots.resolve().is_relative_to(profile.resolve()) and backup.resolve().is_relative_to(profile.resolve())
  screenshots.rename(backup);screenshots.write_text('synthetic save failure',encoding='utf-8')
  exit_request();confirm=find('PanzoMessageDialog',title='退出 Panzo',visible=True);answer(confirm,6)
  wait(lambda:'图片保存失败' in value(api.GetDlgItem(main,10)));wait(lambda:not dialogs());time.sleep(1.3)
  assert len(picture_windows())==2 and process.poll() is None and not dialogs();assert_sources(originals);snapshot(main,'save-failure-retains-documents.png')
  screenshots.unlink();backup.rename(screenshots)
  exit_request();confirm=find('PanzoMessageDialog',title='退出 Panzo',visible=True);answer(confirm,6);finish();assert len(list(screenshots.glob('截图-*.png')))==4
  report['checks'].append('Save failure cancels exit, reports inline without extra dialogs, retains all documents and permits a later successful retry')
  launch();exit_request();finish();report['checks'].append('clean workspace exits without a confirmation')
  time.sleep(2)
  import re
  report['newCrashDumps']=[p.name for p in dump_dir.glob('panzo*.dmp') if (match:=re.search(r'\.(\d+)\.dmp$',p.name)) and int(match[1]) in report['processIds'] and dumps_before.get(p.name)!=p.stat().st_mtime_ns]
  assert not report['newCrashDumps'],report['newCrashDumps']
  report['checks'].append('all test processes exit without new crash dumps, including normal painting teardown')
  report['passed']=True
finally:
 if process is not None:
  report['lastProcessId']=process.pid;report['exitCode']=process.poll()
  if process.poll() is None:
   try:
    if not report['passed'] and main and api.IsWindow(main):snapshot(main,'failure.png')
   finally:
    if process.poll() is None:process.kill();process.wait()
 (out/'exit-smoke.json').write_text(json.dumps(report,ensure_ascii=False,indent=2),encoding='utf-8')
print(json.dumps(report,ensure_ascii=False))

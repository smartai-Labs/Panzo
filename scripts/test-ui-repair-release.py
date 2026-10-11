"""Native release regression. Uses an isolated profile; never closes existing Panzo processes."""
import argparse, ctypes as c, json, os, subprocess, time, uuid
from ctypes import wintypes as w
from pathlib import Path
from PIL import Image, ImageGrab
args=argparse.ArgumentParser();args.add_argument('--binary',default='target/desktop-v4/release/panzo.exe');args.add_argument('--output',default='docs/acceptance/ui-repair-20261009/release');opt=args.parse_args()
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
 time.sleep(.25);r=w.RECT();api.GetWindowRect(h,c.byref(r));ImageGrab.grab(bbox=(r.left,r.top,r.right,r.bottom)).save(out/name)
startup=subprocess.STARTUPINFO();startup.dwFlags|=subprocess.STARTF_USESHOWWINDOW;startup.wShowWindow=0
env=os.environ.copy();env['LOCALAPPDATA']=str(profile);env['USERPROFILE']=str(profile)
process=subprocess.Popen([str(root/opt.binary)],env=env,startupinfo=startup)
report={'passed':False,'binary':opt.binary,'checks':[],'processId':process.pid};main=None
try:
 main=find('PanzoWorkspace');wait(lambda:api.GetDlgItem(main,116));api.SetWindowPos(main,w.HWND(-1),30,30,1180,860,0x40)
 assert [value(api.GetDlgItem(main,i)) for i in range(100,103)]==['图片工具','录制与视频','素材']
 assert [value(api.GetDlgItem(main,i)) for i in range(111,117)]==['文件','编辑','视图','帮助','插件','设置']
 assert not api.GetDlgItem(main,103) and not api.GetDlgItem(main,104)
 report['checks'].append('three workspaces; ordered top commands; no settings/plugin tabs')
 snapshot(main,'workspace-dark-empty.png')
 before=value(main);command(main,116);settings=find('PanzoManagementWindow',title='设置',visible=True)
 api.SetWindowPos(settings,w.HWND(-1),90,70,960,740,0x40)
 form=find('PanzoWorkspaceForm',settings,visible=True);folder=api.GetDlgItem(form,102);assert folder
 original=value(folder);entry=api.GetDlgItem(folder,3);assert not api.IsWindowVisible(entry)
 command(folder,2);assert api.IsWindowVisible(entry);set_text(entry,str(profile/'取消的目录'));api.PostMessageW(entry,0x100,27,0)
 wait(lambda:not api.IsWindowVisible(entry));assert value(folder)==original
 command(folder,2);set_text(entry,str(profile/'手动目录'));api.PostMessageW(entry,0x100,13,0)
 wait(lambda:not api.IsWindowVisible(entry) and value(folder)==str(profile/'手动目录'))
 api.PostMessageW(folder,0x111,1,0);picker=find('#32770',visible=True);api.PostMessageW(picker,0x111,2,0)
 wait(lambda:not api.IsWindowVisible(picker));assert value(folder)==str(profile/'手动目录')
 report['checks'].append('directory field is transient; posted Escape/Enter; native picker cancel preserves path')
 snapshot(settings,'settings-directory-dark.png')
 command(settings,121);service=find('PanzoWorkspaceForm',settings,visible=True);url=api.GetDlgItem(service,100);model=api.GetDlgItem(service,101)
 set_text(url,'https://example.invalid/v1');set_text(model,'unsaved-test-model')
 command(settings,120);command(settings,121);service2=find('PanzoWorkspaceForm',settings,visible=True)
 assert service2==service and value(api.GetDlgItem(service2,101))=='unsaved-test-model', {'first':service,'second':service2,'value':value(api.GetDlgItem(service2,101)),'headings':[(h,value(api.GetDlgItem(h,10)),bool(api.IsWindowVisible(h))) for h,k,t in windows(settings) if k=='PanzoWorkspaceForm']}
 for id in [100,101,102]:
  edit=api.GetDlgItem(service,id);assert api.GetWindowLongPtrW(edit,-16)&0x800000==0;assert api.GetWindowLongPtrW(edit,-20)&0x200==0
 api.PostMessageW(settings,0x10,0,0);wait(lambda:not api.IsWindowVisible(settings));command(main,116)
 assert find('PanzoManagementWindow',title='设置',visible=True)==settings and value(main)==before
 report['checks'].append('settings drafts survive navigation and closing; popup is unique; main workspace retained; no EDIT frames')
 command(main,115);plugins=find('PanzoManagementWindow',title='插件',visible=True);command(main,115)
 assert len([h for h,k,t in windows() if k=='PanzoManagementWindow'])==2
 api.PostMessageW(settings,0x10,0,0);api.SetWindowPos(plugins,w.HWND(-1),90,70,900,640,0x40);snapshot(plugins,'plugins-dark.png');api.PostMessageW(plugins,0x10,0,0);api.PostMessageW(settings,0x10,0,0)
 report['checks'].append('settings/plugins each use a unique owned window')
 command(main,102);history=find('PanzoCaptureHistory',main);time.sleep(1.0);command(history,12);time.sleep(.3);snapshot(main,'assets-dark.png');command(history,3)
 picture=find('PanzoPicture',main,visible=True);panel=find('PanzoImageWorkspace',main)
 result=api.GetDlgItem(panel,25);set_text(result,'\r\n'.join(f'第 {i} 行 · 用于滚动条与识别输出显示验收' for i in range(1,65)))
 assert api.GetWindowLongPtrW(result,-16)&(0x800000|0x200000)==0
 time.sleep(2);snapshot(main,'image-dark.png');command(picture,36);snapshot(main,'image-crop-dark.png');command(picture,62)
 command(main,110);snapshot(main,'image-light.png')
 command(main,116);command(settings,120);snapshot(settings,'settings-light.png');api.PostMessageW(settings,0x10,0,0)
 report['checks'].append('asset preview and image editing load; crop session; light/dark controls; result scrollbar has no native white frame')
 thumb=api.GetDlgItem(panel,101);command(picture,40);wait(lambda:'1000 × 1600' in value(thumb));edited=value(thumb)
 command(main,101);command(main,207);command(main,100);assert value(thumb)==edited
 command(picture,7);wait(lambda:'1600 × 1000' in value(thumb));command(picture,44)
 report['checks'].append('video-page undo leaves rotated image unchanged; image-page undo restores original dimensions')
 api.PostMessageW(main,0x10,0,0);process.wait(timeout=10);assert process.returncode==0
 report['passed']=True;report['exitCode']=process.returncode
finally:
 if process.poll() is None:
  if main:api.PostMessageW(main,0x10,0,0)
  try:process.wait(timeout=3)
  except subprocess.TimeoutExpired:process.kill();process.wait()
 (out/'ui-smoke.json').write_text(json.dumps(report,ensure_ascii=False,indent=2),encoding='utf-8')
print(json.dumps(report,ensure_ascii=False))

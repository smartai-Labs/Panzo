"""Windows release UI smoke test; requires Pillow. Uses an isolated profile and closes only its own process."""
import ctypes as c
from ctypes import wintypes as w
import json, os, subprocess, time
from pathlib import Path
from PIL import ImageGrab
root=Path(__file__).resolve().parent.parent
out=root/'docs/acceptance/desktop-v3-native-20261008/final'
out.mkdir(parents=True,exist_ok=True)
profile=root/'.tmp/desktop-v3-release-smoke-profile'
(profile/'Panzo').mkdir(parents=True,exist_ok=True)
(profile/'Panzo/settings.json').write_text(json.dumps({'theme':'dark','close_to_tray':False}),encoding='utf-8')
env=os.environ.copy();env['LOCALAPPDATA']=str(profile);env['USERPROFILE']=str(profile)
api=c.WinDLL('user32',use_last_error=True)
api.GetWindowThreadProcessId.argtypes=[w.HWND,c.POINTER(w.DWORD)]
api.GetClassNameW.argtypes=[w.HWND,w.LPWSTR,c.c_int]
api.GetWindowTextW.argtypes=[w.HWND,w.LPWSTR,c.c_int]
api.GetDlgItem.argtypes=[w.HWND,c.c_int];api.GetDlgItem.restype=w.HWND
api.IsWindowVisible.argtypes=[w.HWND];api.IsWindowVisible.restype=w.BOOL
api.SendMessageTimeoutW.argtypes=[w.HWND,w.UINT,c.c_size_t,c.c_ssize_t,w.UINT,w.UINT,c.POINTER(c.c_size_t)]
api.SendMessageTimeoutW.restype=c.c_size_t
api.GetWindowRect.argtypes=[w.HWND,c.POINTER(w.RECT)]
callback=c.WINFUNCTYPE(w.BOOL,w.HWND,w.LPARAM)
api.EnumWindows.argtypes=[callback,w.LPARAM]
api.PostMessageW.argtypes=[w.HWND,w.UINT,c.c_size_t,c.c_ssize_t]
def text(h):
 b=c.create_unicode_buffer(512);api.GetWindowTextW(h,b,len(b));return b.value
def message(h,msg,wp=0,lp=0):
 result=c.c_size_t()
 if not api.SendMessageTimeoutW(h,msg,wp,lp,2,2000,c.byref(result)):raise RuntimeError('window message timed out')
 return result.value
def windows(pid):
 result=[]
 @callback
 def each(h,_):
  owner=w.DWORD();api.GetWindowThreadProcessId(h,c.byref(owner))
  if owner.value==pid:
   name=c.create_unicode_buffer(100);api.GetClassNameW(h,name,len(name))
   result.append((h,name.value,text(h)))
  return True
 api.EnumWindows(each,0);return result
startup=subprocess.STARTUPINFO();startup.dwFlags|=subprocess.STARTF_USESHOWWINDOW;startup.wShowWindow=0
process=subprocess.Popen([str(root/'target/desktop-v3/release/panzo.exe')],env=env,startupinfo=startup)
report={'passed':False,'pid':process.pid,'binary':'target/desktop-v3/release/panzo.exe'}
hwnd=None
try:
 end=time.monotonic()+15
 while time.monotonic()<end:
  owned=windows(process.pid)
  found=[h for h,kind,_ in owned if kind=='PanzoWorkspace']
  if found and api.GetDlgItem(found[0],100):
   hwnd=found[0];break
  if process.poll() is not None:raise RuntimeError('app exited before workspace appeared')
  time.sleep(.1)
 if not hwnd:raise RuntimeError('workspace missing: '+str(owned))
 time.sleep(.8)
 report['navigation']=[text(api.GetDlgItem(hwnd,i)) for i in range(100,105)]
 assert report['navigation']==['图片工具','录制与视频','素材库','设置','插件'],report
 for i in [1,2,4,3,0]:
  message(hwnd,0x111,100+i);time.sleep(.18)
  assert report['navigation'][i] in text(hwnd),text(hwnd)
 time.sleep(.3)
 rect=w.RECT();assert api.GetWindowRect(hwnd,c.byref(rect))
 assert api.IsWindowVisible(hwnd)
 bitmap=ImageGrab.grab(bbox=(rect.left,rect.top,rect.right,rect.bottom))
 bitmap.save(out/'release-startup.png')
 report['navigationTextPixels']={}
 for control in [101,102,104]:
  child_rect=w.RECT();api.GetWindowRect(api.GetDlgItem(hwnd,control),c.byref(child_rect))
  count=sum(1 for y in range(child_rect.top-rect.top+6,child_rect.bottom-rect.top-6) for x in range(child_rect.left-rect.left+8,child_rect.right-rect.left-8) if min(bitmap.getpixel((x,y))[:3])>150)
  report['navigationTextPixels'][str(control)]=count
  assert count>20, f'flat navigation label missing: {control}/{count}'
 report['windowTitle']=text(hwnd)
 report['windowSize']=[rect.right-rect.left,rect.bottom-rect.top]
 api.PostMessageW(hwnd,0x10,0,0)
 process.wait(timeout=8)
 report['exitCode']=process.returncode
 assert process.returncode==0
 report['passed']=True
finally:
 if process.poll() is None:
  if hwnd:api.PostMessageW(hwnd,0x10,0,0)
  try:process.wait(timeout=3)
  except subprocess.TimeoutExpired:process.kill();process.wait()
 (out/'release-smoke.json').write_text(json.dumps(report,ensure_ascii=False,indent=2),encoding='utf-8')
print(json.dumps(report,ensure_ascii=False))


"""Release-only canvas text/repaint acceptance in a disposable process/profile."""
import argparse,ctypes as c,hashlib,importlib.util,json,os,subprocess,time,uuid
from ctypes import wintypes as w
from pathlib import Path
from PIL import Image,ImageGrab,ImageChops
spec=importlib.util.spec_from_file_location('drawing',Path(__file__).with_name('test-drawing-stability.py'))
d=importlib.util.module_from_spec(spec);spec.loader.exec_module(d)
a=argparse.ArgumentParser();a.add_argument('--pattern',action='store_true');a.add_argument('--binary',type=Path,required=True);a.add_argument('--output',type=Path,required=True);args=a.parse_args()
binary=args.binary.resolve();out=args.output.resolve();out.mkdir(parents=True,exist_ok=True)
n=d.Native();u=n.api
for name,types,ret in [('SetForegroundWindow',[w.HWND],w.BOOL),('ShowWindow',[w.HWND,c.c_int],w.BOOL),('IsHungAppWindow',[w.HWND],w.BOOL),('SetCursorPos',[c.c_int,c.c_int],w.BOOL),('GetCursorPos',[c.POINTER(w.POINT)],w.BOOL),('GetGuiResources',[w.HANDLE,w.DWORD],w.DWORD)]:
 f=getattr(u,name);f.argtypes=types;f.restype=ret
profile=Path('.tmp').resolve()/('text-release-'+str(uuid.uuid4()));(profile/'Panzo').mkdir(parents=True)
fixture=profile/'Videos/Panzo/Screenshots';fixture.mkdir(parents=True)
(profile/'Panzo/settings.json').write_text(json.dumps({'theme':'dark','close_to_tray':False}),encoding='utf-8')
from PIL import ImageDraw, ImageFont
fixture_image=Image.new('RGB',(960,540),'white')
draw=ImageDraw.Draw(fixture_image);font=ImageFont.truetype('C:/Windows/Fonts/arial.ttf',54)
draw.text((80,120),'PANZO OCR TEST',font=font,fill='black')
draw.text((80,210),'Text stays available',font=font,fill='black')
fixture_image.save(fixture/'text-fixture.png')

env=os.environ.copy();env.update(LOCALAPPDATA=str(profile),USERPROFILE=str(profile),APPDATA=str(profile/'Roaming'))
startup=subprocess.STARTUPINFO();startup.dwFlags|=subprocess.STARTF_USESHOWWINDOW;startup.wShowWindow=0
p=root=None;cursor=w.POINT();u.GetCursorPos(c.byref(cursor))
r={'binary':str(binary),'sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'passed':False}
def value(h):
 b=c.create_unicode_buffer(4097);n.send(h,0xD,4097,c.addressof(b));return b.value
def type_text(h,s):
 b=c.create_unicode_buffer(s);n.send(h,0xB1,0,-1);n.send(h,0xC2,1,c.addressof(b))
def click(h,x,y):
 xx,yy,_,_=n.geometry(h);u.SetCursorPos(xx+x,yy+y)
 n.send(h,0x201,1,d.packed_point(x,y));n.send(h,0x202,0,d.packed_point(x,y))
def snap(name):
 foreground(root)
 if 'toast' in globals():n.send(toast,0x113,1,0)
 image=ImageGrab.grab(bbox=n.outer(root),all_screens=True);image.save(out/name);return image

u.GetForegroundWindow.restype=w.HWND
u.AttachThreadInput.argtypes=[w.DWORD,w.DWORD,w.BOOL];u.AttachThreadInput.restype=w.BOOL
k=c.WinDLL('kernel32');k.GetCurrentThreadId.restype=w.DWORD
def foreground(h):
 current=k.GetCurrentThreadId();other=u.GetWindowThreadProcessId(u.GetForegroundWindow(),None)
 attached=current!=other and u.AttachThreadInput(current,other,True)
 try:u.SetForegroundWindow(h)
 finally:
  if attached:u.AttachThreadInput(current,other,False)
def wait_until(fn,seconds=10):
 end=time.monotonic()+seconds
 while time.monotonic()<end:
  if fn():return
  d.check_alive(p);time.sleep(.02)
 raise AssertionError('Timed out waiting for notification state')
try:
 p=subprocess.Popen([str(binary)],env=env,startupinfo=startup);r['isolatedPid']=p.pid
 root=n.find(p,'PanzoWorkspace',20);u.SetWindowPos(root,w.HWND(-1),80,80,1180,850,0x40);foreground(root)
 n.send(root,d.WM_COMMAND,102);history=n.find(p,'PanzoCaptureHistory',20,root);n.send(history,d.WM_COMMAND,12);d.wait_for_fixture(n,p,history,20);n.send(history,d.WM_COMMAND,3)
 picture=n.find(p,'PanzoPicture',20,root);time.sleep(.4)

 image_workspace=n.find(p,'PanzoImageWorkspace',10,root)
 toast=n.find(p,'PanzoToast',10)
 assert not u.GetDlgItem(root,10),'old bottom status control still exists'
 assert not u.IsWindowVisible(toast)
 n.send(picture,d.WM_COMMAND,46);time.sleep(.1);foreground(root)
 n.send(image_workspace,d.WM_COMMAND,7)
 wait_until(lambda:value(toast)=='离线 OCR已完成',30)
 first=time.monotonic();r['message']=value(toast);assert r['message']=='离线 OCR已完成',r['message']
 time.sleep(.2)
 x,y,width,height=n.geometry(root);l,t,rr,b=n.outer(toast)
 r['toastRect']=[l,t,rr,b];r['rootClient']=[x,y,width,height]
 r['horizontalCenterDelta']=abs((l+rr)/2-(x+width/2));assert r['horizontalCenterDelta']<=1
 assert y<t<y+160 and l>=x and rr<=x+width and b<y+height
 assert u.GetForegroundWindow()!=toast
 snap('01-ocr-success-top.png')
 result=u.GetDlgItem(image_workspace,25);r['recognizedText']=value(result);assert 'PANZO' in r['recognizedText'].upper()
 # Resize and move while visible; the notification stays centered in this app.
 u.SetWindowPos(root,None,120,95,1280,880,0x14);time.sleep(.08)
 x,y,width,height=n.geometry(root);l,t,rr,b=n.outer(toast)
 assert abs((l+rr)/2-(x+width/2))<=1 and y<t<y+160
 wait_until(lambda:not u.IsWindowVisible(toast) and not value(toast),4)
 r['messageLifetimeSeconds']=time.monotonic()-first;assert 2.1<r['messageLifetimeSeconds']<3.3
 r['resultPreserved']=value(result)==r['recognizedText'];assert r['resultPreserved']
 snap('02-after-auto-dismiss.png')
 # Trigger again, switch theme, and verify minimize hides the notification.
 n.send(root,d.WM_COMMAND,110);n.send(image_workspace,d.WM_COMMAND,7)
 wait_until(lambda:value(toast)=='离线 OCR已完成')
 time.sleep(.2);snap('03-ocr-success-light.png')
 u.ShowWindow(root,6);time.sleep(.1);r['hiddenOnMinimize']=not bool(u.IsWindowVisible(toast));assert r['hiddenOnMinimize']
 u.ShowWindow(root,9);foreground(root)
 wait_until(lambda:not u.IsWindowVisible(toast) and not value(toast),4)
 r['hung']=bool(u.IsHungAppWindow(root));assert not r['hung'];r['passed']=True
except Exception as e:
 import traceback;r['error']=traceback.format_exc()
 if root:
  r['foreground']=u.GetForegroundWindow();r['root']=root
  r['windows']=[{'h':h,'class':name,'text':value(h),'visible':bool(u.IsWindowVisible(h))} for h,name in n.windows(p.pid) if name in ('PanzoToast','PanzoWorkspace')]
  r['imageControls']=[{'h':h,'class':name,'text':value(h),'visible':bool(u.IsWindowVisible(h))} for h,name in n.windows(p.pid,image_workspace) if name in ('Static','Edit','STATIC','EDIT')]
  snap('failure.png')
finally:
 u.SetCursorPos(cursor.x,cursor.y)
 if p and p.poll() is None:
  if root:u.PostMessageW(root,d.WM_CLOSE,0,0)
  try:p.wait(timeout=3)
  except subprocess.TimeoutExpired:p.kill();p.wait(timeout=5);r['cleanupForcedOwnProcess']=True
 (out/'report.json').write_text(json.dumps(r,ensure_ascii=False,indent=2),encoding='utf-8');print(json.dumps(r,ensure_ascii=False))
raise SystemExit(0 if r['passed'] else 1)

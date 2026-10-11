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
fixture_image=Image.new('RGB',(3840,2160),(215,222,230))
if args.pattern:
 from PIL import ImageDraw
 draw=ImageDraw.Draw(fixture_image)
 for yy in range(60,300,16):
  for xx in range(70,640,16):
   draw.rectangle((xx,yy,xx+15,yy+15),fill=(175,185,195) if (xx//16+yy//16)%2 else (230,235,240))
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
 image=ImageGrab.grab(bbox=n.outer(root),all_screens=True);image.save(out/name);return image
try:
 p=subprocess.Popen([str(binary)],env=env,startupinfo=startup);r['isolatedPid']=p.pid
 root=n.find(p,'PanzoWorkspace',20);u.SetWindowPos(root,w.HWND(-1),80,80,1180,850,0x40);u.SetForegroundWindow(root)
 n.send(root,d.WM_COMMAND,102);history=n.find(p,'PanzoCaptureHistory',20,root);n.send(history,d.WM_COMMAND,12);d.wait_for_fixture(n,p,history,20);n.send(history,d.WM_COMMAND,3)
 picture=n.find(p,'PanzoPicture',20,root);time.sleep(.4)
 ox,oy,cw,ch=n.geometry(picture)
 canvas=ImageGrab.grab(bbox=(ox,oy,ox+cw,oy+ch),all_screens=True).convert('RGB')
 mask=canvas.point(lambda _:0).convert('L');mask.putdata([255 if rgb==(215,222,230) else 0 for rgb in canvas.getdata()]);bounds=mask.getbbox();assert bounds,'fixture not visible'
 left,top,right,bottom=bounds;x=left+100;y=top+90
 n.send(picture,d.WM_COMMAND,4);click(picture,x,y)
 edit=u.GetDlgItem(picture,2000);side=u.GetDlgItem(picture,20)
 assert edit and u.IsWindowVisible(edit),'no inline editor'

 type_text(edit,'Drag text 文字')
 size=u.GetDlgItem(picture,51);v=c.create_unicode_buffer('64');n.send(size,0xC,0,c.addressof(v))
 u.RedrawWindow.argtypes=[w.HWND,c.POINTER(w.RECT),w.HANDLE,w.UINT];u.RedrawWindow.restype=w.BOOL
 def redraw():u.RedrawWindow(picture,None,None,0x181)
 redraw();time.sleep(.1)
 el,et,er,eb=n.outer(edit);startx=el+(er-el)//4;starty=et+2
 u.SetCursorPos(startx,starty);n.send(edit,0xA1,18,d.packed_point(startx,starty))
 timings=[]
 for i in range(1,61):
  started=time.perf_counter();u.SetCursorPos(startx+i*2,starty+i)
  n.send(edit,0x200,1,0);redraw();timings.append((time.perf_counter()-started)*1000)
 n.send(edit,0x202,0,0);redraw()
 nl,nt,nr,nb=n.outer(edit)
 r['inlineDragDelta']=[nl-el,nt-et]
 r['inlineFrameMs']={'mean':sum(timings)/len(timings),'p95':sorted(timings)[56],'max':max(timings)}
 assert abs(nl-el-120)<=2 and abs(nt-et-60)<=2
 assert value(edit)=='Drag text 文字'
 snap('01-after-inline-drag.png')
 # Start middle panning from inside the live transparent input.
 px=nl-ox+12;py=nt-oy+12
 u.SetCursorPos(ox+px,oy+py)
 n.send(edit,0x207,0x10,d.packed_point(12,12))
 timings=[]
 for i in range(1,31):
  started=time.perf_counter();u.SetCursorPos(ox+px+i*2,oy+py+i);n.send(picture,0x200,0x10,d.packed_point(px+i*2,py+i));redraw();timings.append((time.perf_counter()-started)*1000)
 n.send(picture,0x208,0,d.packed_point(px+60,py+30));redraw()
 al,at,ar,ab=n.outer(edit)
 r['middlePanDelta']=[al-nl,at-nt];assert (al-nl,at-nt)==(60,30)
 r['panFrameMs']={'mean':sum(timings)/len(timings),'p95':sorted(timings)[27],'max':max(timings)}
 r['keptEditing']=bool(u.IsWindowVisible(edit)) and value(edit)=='Drag text 文字';assert r['keptEditing']
 n.send(picture,0x200,0,d.packed_point(px+180,py+120));redraw()
 r['stoppedOnRelease']=n.outer(edit)==(al,at,ar,ab);assert r['stoppedOnRelease']
 snap('02-middle-pan-active-text.png')
 # A blank part of the preview can also initiate panning.
 u.SetCursorPos(ox+55,oy+40)
 n.send(picture,0x207,0x10,d.packed_point(55,40))
 u.SetCursorPos(ox+25,oy+20)
 n.send(picture,0x200,0x10,d.packed_point(25,20));n.send(picture,0x208,0,d.packed_point(25,20));redraw()
 bl,bt,br,bb=n.outer(edit);r['blankCanvasPanDelta']=[bl-al,bt-at];assert (bl-al,bt-at)==(-30,-20)
 r['hung']=bool(u.IsHungAppWindow(root));assert not r['hung']
 r['passed']=True
except Exception as e:
 import traceback;r['error']=traceback.format_exc()
finally:
 u.SetCursorPos(cursor.x,cursor.y)
 if p and p.poll() is None:
  if root:u.PostMessageW(root,d.WM_CLOSE,0,0)
  try:p.wait(timeout=3)
  except subprocess.TimeoutExpired:p.kill();p.wait(timeout=5);r['cleanupForcedOwnProcess']=True
 (out/'report.json').write_text(json.dumps(r,ensure_ascii=False,indent=2),encoding='utf-8');print(json.dumps(r,ensure_ascii=False))
raise SystemExit(0 if r['passed'] else 1)

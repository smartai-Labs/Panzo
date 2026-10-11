"""Regression for property preview reentry, outside deselection and stable text raster."""
import argparse,ctypes as c,hashlib,importlib.util,json,os,subprocess,time,uuid
from ctypes import wintypes as w
from pathlib import Path
from PIL import Image,ImageGrab,ImageChops
spec=importlib.util.spec_from_file_location('drawing',Path(__file__).with_name('test-drawing-stability.py'))
d=importlib.util.module_from_spec(spec);spec.loader.exec_module(d)
a=argparse.ArgumentParser();a.add_argument('--pattern',action='store_true');a.add_argument('--binary',type=Path,required=True);a.add_argument('--output',type=Path,required=True);args=a.parse_args()
binary=args.binary.resolve();out=args.output.resolve();out.mkdir(parents=True,exist_ok=True)
n=d.Native();u=n.api
u.IsWindowEnabled.argtypes=[w.HWND];u.IsWindowEnabled.restype=w.BOOL
for name,types,ret in [('SetForegroundWindow',[w.HWND],w.BOOL),('ShowWindow',[w.HWND,c.c_int],w.BOOL),('IsHungAppWindow',[w.HWND],w.BOOL),('SetCursorPos',[c.c_int,c.c_int],w.BOOL),('GetCursorPos',[c.POINTER(w.POINT)],w.BOOL),('GetGuiResources',[w.HANDLE,w.DWORD],w.DWORD)]:
 f=getattr(u,name);f.argtypes=types;f.restype=ret
profile=Path('.tmp').resolve()/('text-release-'+str(uuid.uuid4()));(profile/'Panzo').mkdir(parents=True)
fixture=profile/'Videos/Panzo/Screenshots';fixture.mkdir(parents=True)
(profile/'Panzo/settings.json').write_text(json.dumps({'theme':'dark','close_to_tray':False}),encoding='utf-8')
fixture_image=Image.new('RGB',(960,540),(215,222,230))
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
 picture=n.find(p,'PanzoPicture',20,root);time.sleep(.3)
 ox,oy,cw,ch=n.geometry(picture)
 canvas=ImageGrab.grab(bbox=(ox,oy,ox+cw,oy+ch),all_screens=True).convert('RGB')
 mask=Image.new('L',canvas.size);mask.putdata([255 if rgb==(215,222,230) else 0 for rgb in canvas.getdata()]);bounds=mask.getbbox();assert bounds
 x,y=bounds[0]+100,bounds[1]+90
 n.send(picture,d.WM_COMMAND,4);click(picture,x,y)
 edit=u.GetDlgItem(picture,2000);family=u.GetDlgItem(picture,2100);size=u.GetDlgItem(picture,51)
 def choose(label):
  buf=c.create_unicode_buffer(label);index=n.send(family,0x14C,-1,c.addressof(buf));assert index!=c.c_size_t(-1).value
  n.send(family,0x14E,index);n.send(picture,d.WM_COMMAND,2100|(1<<16),family)
 choose('Arial')
 custom=c.create_unicode_buffer('48');n.send(size,0xC,0,c.addressof(custom));n.send(picture,d.WM_COMMAND,51|(5<<16),size)
 type_text(edit,'Abc')
 box=n.outer(edit)
 def ink(name):
  time.sleep(.14);im=ImageGrab.grab(bbox=box,all_screens=True).convert('RGB');im.save(out/name)
  mask=Image.new('L',im.size);mask.putdata([255 if v[0]>160 and v[1]<160 and v[2]<160 else 0 for v in im.getdata()]);return mask.getbbox()
 button=u.GetDlgItem(picture,2103)
 for _ in range(2):
  n.send(button,0x201,1,d.packed_point(10,10));n.send(button,0x202,0,d.packed_point(10,10))
 r['editingInk']=ink('editing.png');r['textAtEditCapture']=value(edit)
 n.send(picture,d.WM_COMMAND,4)
 r['editingClosed']=not bool(u.IsWindowVisible(edit))
 r['committedInk']=ink('committed.png');r['textAtCommitCapture']=value(u.GetDlgItem(picture,20))
 r['inkDelta']=[b-a for a,b in zip(r['editingInk'],r['committedInk'])]
 r['initialFamily']=value(family)
 choose('Courier New');n.send(picture,d.WM_COMMAND,2103)
 r['familyAfterInspectorChange']=value(family);ink('property-preview.png')
 click(picture,x+10,y+12)
 r['reenteredEditor']=bool(u.IsWindowVisible(edit));r['familyAfterReenter']=value(family);ink('reentered.png')
 # A click on an empty area currently starts another editor rather than deactivating.
 old=n.outer(edit);click(picture,x+300,y+140)
 r['outsideClickLeavesEditorVisible']=bool(u.IsWindowVisible(edit));r['outsideClickCreatesEmptyDraft']=value(edit)=='';r['oldEditorRect']=old;r['newEditorRect']=n.outer(edit)
 r['passed']=r['editingInk']==r['committedInk'] and r['textAtEditCapture']==r['textAtCommitCapture']=='Abc' and r['familyAfterReenter']=='Courier New' and not r['outsideClickLeavesEditorVisible']
 r['unselectedFontControlDisabled']=not bool(u.IsWindowEnabled(family))
 r['passed'] &= r['unselectedFontControlDisabled']
except Exception:
 import traceback;r['error']=traceback.format_exc()
finally:
 u.SetCursorPos(cursor.x,cursor.y)
 if p and p.poll() is None:
  if root:u.PostMessageW(root,d.WM_CLOSE,0,0)
  try:p.wait(timeout=3)
  except subprocess.TimeoutExpired:p.kill();p.wait(timeout=5)
 (out/'report.json').write_text(json.dumps(r,ensure_ascii=False,indent=2),encoding='utf-8');print(json.dumps(r,ensure_ascii=False))
raise SystemExit(0 if r['passed'] else 1)

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
 picture=n.find(p,'PanzoPicture',20,root);time.sleep(.4)
 ox,oy,cw,ch=n.geometry(picture)
 canvas=ImageGrab.grab(bbox=(ox,oy,ox+cw,oy+ch),all_screens=True).convert('RGB')
 mask=canvas.point(lambda _:0).convert('L');mask.putdata([255 if rgb==(215,222,230) else 0 for rgb in canvas.getdata()]);bounds=mask.getbbox();assert bounds,'fixture not visible'
 left,top,right,bottom=bounds;x=left+100;y=top+90
 n.send(picture,d.WM_COMMAND,4);click(picture,x,y)
 edit=u.GetDlgItem(picture,2000);side=u.GetDlgItem(picture,20)
 assert edit and u.IsWindowVisible(edit),'no inline editor'
 r['emptyDraft']=value(edit)==''
 class GUI(c.Structure):
  _fields_=[('cbSize',w.DWORD),('flags',w.DWORD),('hwndActive',w.HWND),('hwndFocus',w.HWND),('hwndCapture',w.HWND),('hwndMenuOwner',w.HWND),('hwndMoveSize',w.HWND),('hwndCaret',w.HWND),('rcCaret',w.RECT)]
 u.GetGUIThreadInfo.argtypes=[w.DWORD,c.POINTER(GUI)];u.GetGUIThreadInfo.restype=w.BOOL
 info=GUI();info.cbSize=c.sizeof(info);pid=w.DWORD();thread=u.GetWindowThreadProcessId(edit,c.byref(pid));u.GetGUIThreadInfo(thread,c.byref(info))
 r['emptyFocus']={'edit':edit,'focus':info.hwndFocus,'caret':info.hwndCaret,'caretRect':[info.rcCaret.left,info.rcCaret.top,info.rcCaret.right,info.rcCaret.bottom],'selection':n.send(edit,0xB0)}
 snap('00-empty-before.png')
 r['emptyCaretVisible']=False
 for attempt in range(8):
  time.sleep(.08)
  region=ImageGrab.grab(bbox=n.outer(edit),all_screens=True).convert('RGB')
  dark=sum(max(rgb)<55 for rgb in region.crop((4,4,12,min(region.height-5,40))).getdata())
  r['emptyCaretVisible'] |= dark>6
  if dark>6:snap('00-empty-caret.png');break
 type_text(edit,'Temporary');type_text(edit,'')
 r['deletedTextCaretVisible']=False
 for attempt in range(8):
  time.sleep(.08);region=ImageGrab.grab(bbox=n.outer(edit),all_screens=True).convert('RGB')
  r['deletedTextCaretVisible'] |= sum(max(rgb)<55 for rgb in region.crop((4,4,12,min(region.height-5,40))).getdata())>6
  if r['deletedTextCaretVisible']:break
 r['fontPreviewHidden']=not bool(u.IsWindowVisible(u.GetDlgItem(picture,2105)))
 size=u.GetDlgItem(picture,51)
 r['sizePresets']=n.send(size,0x146)
 assert r['sizePresets']>=17
 custom=c.create_unicode_buffer('37');n.send(size,0xC,0,c.addressof(custom));n.send(picture,d.WM_COMMAND,51|(5<<16),size)
 snap('00b-custom-size.png')
 custom=c.create_unicode_buffer('24');n.send(size,0xC,0,c.addressof(custom));n.send(picture,d.WM_COMMAND,51|(5<<16),size)

 def choose(control,label,command):
  buf=c.create_unicode_buffer(label);index=n.send(control,0x14C,-1,c.addressof(buf))
  assert index!=c.c_size_t(-1).value, f'font option missing: {label}'
  n.send(control,0x14E,index);n.send(picture,d.WM_COMMAND,command|(1<<16),control)
 choose(u.GetDlgItem(picture,2100),'Arial',2100)
 n.send(picture,d.WM_COMMAND,2101)
 r['selectedFamily']=value(u.GetDlgItem(picture,2100));r['selectedWeight']=value(u.GetDlgItem(picture,2101))

 # WM_CHAR is the message generated by normal keyboard typing, including Unicode.
 for char in '直接输入中文':n.send(edit,0x102,ord(char))
 r['keyboardSynchronized']=value(side)=='直接输入中文'
 type_text(edit,'画布输入\r\n第二行');r['canvasToInspector']=value(side)=='画布输入\r\n第二行'
 type_text(side,'右侧同步修改');r['inspectorToCanvas']=value(edit)=='右侧同步修改'
 time.sleep(.15);snap('01-inline-entry.png')
 el,et,er,eb=n.outer(edit)
 bg=ImageGrab.grab(bbox=(el+6,eb-15,er-6,eb-11),all_screens=True).convert('RGB')
 expected_bg=canvas.crop((el+6-ox,eb-15-oy,er-6-ox,eb-11-oy))
 r['transparentPixels']=sum(a==b for a,b in zip(bg.getdata(),expected_bg.getdata()));r['backgroundPixels']=bg.width*bg.height
 r['transparentBackground']=r['transparentPixels']>=r['backgroundPixels']*.9
 full_ink=ImageGrab.grab(bbox=n.outer(edit),all_screens=True).convert('RGB')
 opacity=u.GetDlgItem(picture,54);alpha=c.create_unicode_buffer('50');n.send(opacity,0xC,0,c.addressof(alpha));time.sleep(.12);snap('01b-half-opacity.png')
 half_ink=ImageGrab.grab(bbox=n.outer(edit),all_screens=True).convert('RGB')
 pairs=[(a,b) for a,b in zip(full_ink.getdata(),half_ink.getdata()) if a[0]>180 and a[1]<160 and a[2]<160]
 r['opacitySamplePixels']=len(pairs)
 r['opacityChanged']=len(pairs)>20 and sum(b[1]-a[1] for a,b in pairs)/len(pairs)>20
 alpha=c.create_unicode_buffer('100');n.send(opacity,0xC,0,c.addressof(alpha))

 # Resize the east handle; then restore width and height by its opposite motion.
 el,et,er,eb=n.outer(edit);width_before=er-el
 u.SetCursorPos(er-4,(et+eb)//2);n.send(edit,0xA1,18,d.packed_point(er-4,(et+eb)//2));u.SetCursorPos(er+66,(et+eb)//2);n.send(edit,0x200,1,0);n.send(edit,0x202,0,0)
 newbox=n.outer(edit);r['resizeExpanded']=newbox[2]-newbox[0]>width_before+50
 r['resizeKeptFontSize']=value(size)=='24'
 snap('01c-region-resized.png')
 # Drag a frame segment between handles to move the live input.

 el,et,er,eb=n.outer(edit);u.SetCursorPos(el+(er-el)//4,et+2);n.send(edit,0xA1,18,d.packed_point(el+(er-el)//4,et+2))
 u.SetCursorPos(el+(er-el)//4+70,et+37);n.send(edit,0x200,1,d.packed_point(71,36));n.send(edit,0x202,0,d.packed_point(71,36))
 nl,nt,nr,nb=n.outer(edit);r['draftMoved']=(nl-el)>40 and (nt-et)>20
 n.send(picture,d.WM_COMMAND,30);r['committedOnToolChange']=not bool(u.IsWindowVisible(edit));time.sleep(.12)
 # Double-click the actual moved text to edit it again.
 nx=nl-ox+5;ny=nt-oy+5;n.send(picture,0x203,0,d.packed_point(nx,ny))
 r['reopenedText']=value(edit);assert u.IsWindowVisible(edit)
 time.sleep(.1)
 base=ImageGrab.grab(bbox=n.outer(edit),all_screens=True).convert('RGB')
 for repeat in range(8):
  n.send(picture,d.WM_COMMAND,2102);n.send(picture,d.WM_COMMAND,2103);n.send(picture,d.WM_COMMAND,2102);n.send(picture,d.WM_COMMAND,2103)
 time.sleep(.1)
 base.save(out/'ghost-base.png')
 after=ImageGrab.grab(bbox=n.outer(edit),all_screens=True).convert('RGB');after.save(out/'ghost-after.png')
 def redmask(im):return [p[0]>160 and p[1]<160 and p[2]<160 for p in im.getdata()]
 r['styleRoundtripGhostFree']=base.size==after.size and redmask(base)==redmask(after)
 snap('01d-style-roundtrip.png')

 type_text(edit,'取消此次修改');u.PostMessageW(edit,0x100,27,0);time.sleep(.15)
 r['escapeClosed']=not bool(u.IsWindowVisible(edit))
 n.send(picture,0x203,0,d.packed_point(nx,ny));r['cancelRestored']=value(edit)=='右侧同步修改'
 type_text(edit,'最终文字');n.send(picture,d.WM_COMMAND,30)
 n.send(picture,0x203,0,d.packed_point(nx,ny));r['editPersisted']=value(edit)=='最终文字'
 n.send(picture,d.WM_COMMAND,30)
 # Selecting a text object keeps the right inspector editable.
 click(picture,nx,ny);r['selectedInspectorVisible']=bool(u.IsWindowVisible(side));type_text(side,'选中后修改');n.send(picture,d.WM_COMMAND,3)
 n.send(picture,0x203,0,d.packed_point(nx,ny));r['selectedInspectorPersisted']=value(edit)=='选中后修改'
 r['selectedInspectorReopenedValue']=value(edit)
 n.send(picture,d.WM_COMMAND,30)
 before=set(profile.rglob('*.png'));n.send(picture,d.WM_COMMAND,44);saved=set(profile.rglob('*.png'))-before
 r['savedTextPixels']=max([sum(1 for rgb,source in zip(Image.open(path).convert('RGB').getdata(),fixture_image.getdata()) if rgb!=source) for path in saved]+[0]);assert r['savedTextPixels']>50
 time.sleep(.15);snap('02-text-committed.png')
 # Record valid control backgrounds before any minimize/restore cycle.
 expected={}
 for tool in [30,3,4]:
  n.send(picture,d.WM_COMMAND,tool);time.sleep(.08)
  for bid in [30,36,1,2,3,4,5,46,67]:
   h=u.GetDlgItem(picture,bid)
   if not h or not u.IsWindowVisible(h):continue
   from collections import Counter
   rgb=ImageGrab.grab(bbox=n.outer(h),all_screens=True).convert('RGB')
   expected.setdefault(bid,set()).add(Counter(rgb.getdata()).most_common(1)[0][0])
 r['gdiBefore']=u.GetGuiResources(w.HANDLE(int(p._handle)),0)
 r['repaintChecks']=0;r['whiteBlocks']=[];r['wrongBackgrounds']=[]
 for iteration in range(30):
  n.send(picture,d.WM_COMMAND,[30,3,4][iteration%3])
  if iteration%5==0:
   u.ShowWindow(root,6);u.ShowWindow(root,9);u.SetForegroundWindow(root)
  time.sleep(.025)
  frame=ImageGrab.grab(bbox=n.outer(root),all_screens=True).convert('RGB');rl,rt,rr,rb=n.outer(root)
  for bid in [30,36,1,2,3,4,5,46,67]:
   h=u.GetDlgItem(picture,bid)
   if not h or not u.IsWindowVisible(h):continue
   l,t,rr,bb=n.outer(h);crop=frame.crop((l-rl,t-rt,rr-rl,bb-rt));pixels=list(crop.getdata())
   if pixels and sum(v==(255,255,255) for v in pixels)>len(pixels)*.5:r['whiteBlocks'].append([iteration,bid])
   if pixels and sum(rgb in expected[bid] for rgb in pixels)<len(pixels)*.65:r['wrongBackgrounds'].append([iteration,bid])
   r['repaintChecks']+=1
 r['gdiAfter']=u.GetGuiResources(w.HANDLE(int(p._handle)),0);r['hung']=bool(u.IsHungAppWindow(root));snap('03-after-repaints.png')
 n.send(picture,d.WM_COMMAND,3)
 for width in [4,24]:
  h=u.GetDlgItem(picture,50);b=c.create_unicode_buffer(str(width));n.send(h,0xC,0,c.addressof(b));time.sleep(.1);snap(f'04-brush-{width}.png')
  r.setdefault('numberFields',[]).append({'value':value(h),'outer':n.outer(h),'client':n.geometry(h),'visible':bool(u.IsWindowVisible(h))})
  xx,yy,ww,hh=n.geometry(h);digits=ImageGrab.grab(bbox=(xx,yy,xx+ww,yy+hh),all_screens=True).convert('RGB')
  ink=sum(min(rgb)>120 for rgb in digits.getdata());r['numberFields'][-1]['visibleInkPixels']=ink
  assert ink>3,'numeric value was not repainted after change'
 checks=['deletedTextCaretVisible','styleRoundtripGhostFree','emptyCaretVisible','fontPreviewHidden','resizeExpanded','resizeKeptFontSize','opacityChanged','transparentBackground','emptyDraft','keyboardSynchronized','canvasToInspector','inspectorToCanvas','draftMoved','committedOnToolChange','escapeClosed','cancelRestored','editPersisted','selectedInspectorVisible','selectedInspectorPersisted']
 r['passed']=all(r.get(k) for k in checks) and not r['whiteBlocks'] and not r['wrongBackgrounds'] and not r['hung'] and r['gdiAfter']<=r['gdiBefore']+5
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

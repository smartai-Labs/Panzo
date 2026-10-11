"""Nonmodal colour, recent history and inspector acceptance against a release binary.
Only launches/closes its own process, using a disposable user profile.
"""
import argparse,ctypes as c,hashlib,importlib.util,json,os,subprocess,time,uuid,threading
from ctypes import wintypes as w
from pathlib import Path
from PIL import Image,ImageGrab
spec=importlib.util.spec_from_file_location('drawing',Path(__file__).with_name('test-drawing-stability.py'))
d=importlib.util.module_from_spec(spec);spec.loader.exec_module(d)
a=argparse.ArgumentParser();a.add_argument('--binary',type=Path,required=True);a.add_argument('--output',type=Path,required=True);args=a.parse_args()
binary=args.binary.resolve();out=args.output.resolve();out.mkdir(parents=True,exist_ok=True)
n=d.Native();u=n.api
for name,typ,ret in [('IsWindowEnabled',[w.HWND],w.BOOL),('IsHungAppWindow',[w.HWND],w.BOOL),('IsWindow',[w.HWND],w.BOOL),('GetWindow',[w.HWND,w.UINT],w.HWND),('SetForegroundWindow',[w.HWND],w.BOOL),('SetCursorPos',[c.c_int,c.c_int],w.BOOL),('GetCursorPos',[c.POINTER(w.POINT)],w.BOOL),('mouse_event',[w.DWORD,w.DWORD,w.DWORD,w.DWORD,c.c_size_t],None)]:
 f=getattr(u,name);f.argtypes=typ;f.restype=ret
class MI(c.Structure):_fields_=[('size',w.DWORD),('monitor',w.RECT),('work',w.RECT),('flags',w.DWORD)]
mc=c.WINFUNCTYPE(w.BOOL,w.HANDLE,w.HDC,c.POINTER(w.RECT),w.LPARAM)
u.GetMonitorInfoW.argtypes=[w.HANDLE,c.POINTER(MI)];u.GetMonitorInfoW.restype=w.BOOL
monitors=[]
@mc
def monitor(h,dc,r,l):
 m=MI();m.size=c.sizeof(m);u.GetMonitorInfoW(h,c.byref(m));monitors.append({'work':[m.work.left,m.work.top,m.work.right,m.work.bottom],'primary':bool(m.flags&1)});return True
u.EnumDisplayMonitors(None,None,monitor,0)
profile=Path('.tmp').resolve()/('recent-release-'+str(uuid.uuid4()));(profile/'Panzo').mkdir(parents=True)
fixture=profile/'Videos/Panzo/Screenshots';fixture.mkdir(parents=True)
settings=profile/'Panzo/settings.json'
seed=['#ABCDEF','#654321','#118855','#7C3CE0','#DA4555','#DA9C23']
settings.write_text(json.dumps({'theme':'dark','close_to_tray':False,'recent_colors':seed}),encoding='utf-8')
Image.new('RGB',(960,540),(215,222,230)).save(fixture/'color-fixture.png')
env=os.environ.copy();env.update(LOCALAPPDATA=str(profile),USERPROFILE=str(profile),APPDATA=str(profile/'Roaming'))
startup=subprocess.STARTUPINFO();startup.dwFlags|=subprocess.STARTF_USESHOWWINDOW;startup.wShowWindow=0
p=root=popup=None;stop=None;sampler=None;cursor=w.POINT();u.GetCursorPos(c.byref(cursor))
r={'binary':str(binary),'sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'profile':str(profile),'cases':[],'passed':False}
def recent():return json.loads(settings.read_text(encoding='utf-8'))['recent_colors']
def value(h):
 b=c.create_unicode_buffer(64);n.send(h,0xD,64,c.addressof(b));return b.value
def text(h,t):
 b=c.create_unicode_buffer(t);n.send(h,0xC,0,c.addressof(b))
def swatch(h):
 _,_,width,height=n.geometry(h);dc=u.GetDC(h)
 try:return n.gdi.GetPixel(dc,width//2,height//2)
 finally:u.ReleaseDC(h,dc)
def wait_closed(h):
 end=time.monotonic()+3
 while u.IsWindow(h) and time.monotonic()<end:time.sleep(.02)
 assert not u.IsWindow(h),'popup did not dismiss'
def open_popup():
 n.send(u.GetDlgItem(picture,38),0xF5)
 h=n.find(p,'PanzoColorPopover',5);u.SetWindowPos(h,w.HWND(-1),0,0,0,0,0x43);time.sleep(.12);return h
def physical_click(h):
 l,t,rr,b=n.outer(h);assert u.WindowFromPoint(w.POINT((l+rr)//2,(t+b)//2))==h,'test target occluded';u.SetCursorPos((l+rr)//2,(t+b)//2);u.mouse_event(2,0,0,0,0);time.sleep(.1);u.mouse_event(4,0,0,0,0)
def launch():
 global p,root,picture
 p=subprocess.Popen([str(binary)],env=env,startupinfo=startup);root=n.find(p,'PanzoWorkspace',20)
 u.SetWindowPos(root,w.HWND(-1),80,80,1180,850,0x40);u.SetForegroundWindow(root)
 n.send(root,d.WM_COMMAND,102);hist=n.find(p,'PanzoCaptureHistory',20,root);n.send(hist,d.WM_COMMAND,12);d.wait_for_fixture(n,p,hist,20);n.send(hist,d.WM_COMMAND,3)
 picture=n.find(p,'PanzoPicture',20,root);time.sleep(.2)
def end_process():
 if p and p.poll() is None:
  if root:u.PostMessageW(root,d.WM_CLOSE,0,0)
  try:p.wait(timeout=3)
  except subprocess.TimeoutExpired:p.kill();p.wait(timeout=5)
try:
 launch();r['isolatedPid']=p.pid
 for case,m in enumerate(sorted(monitors,key=lambda m:m['primary'])):
  n.send(picture,d.WM_COMMAND,3)
  l,t,rr,b=m['work'];u.SetWindowPos(root,w.HWND(-1),l+12,t+12,min(1180,rr-l-24),min(850,b-t-24),0x40);u.SetForegroundWindow(root);time.sleep(.2)
  popup=open_popup();before=recent();pl,pt,pr,pb=n.outer(popup);rl,rt,rr,rb=n.outer(root)
  row={'monitor':m,'popup':[pl,pt,pr,pb],'root':[rl,rt,rr,rb]};r['cases'].append(row)
  row['rootEnabled']=bool(u.IsWindowEnabled(root));row['ownedByRoot']=u.GetWindow(popup,4)==root
  row['insideRoot']=rl<=pl<pr<=rr and rt<=pt<pb<=rb
  row['noApplyCancel']=not u.GetDlgItem(popup,1) and not u.GetDlgItem(popup,2)
  scale=(pr-pl)/292;point=lambda x,y:d.packed_point(round(x*scale),round(y*scale))
  samples=[];stop=threading.Event()
  def sample():
   dc=u.GetDC(popup)
   try:
    while not stop.is_set():samples.append(n.gdi.GetPixel(dc,round(8*scale),round(30*scale)));time.sleep(.001)
   finally:u.ReleaseDC(popup,dc)
  sampler=threading.Thread(target=sample);sampler.start()
  n.send(popup,0x201,1,point(40,240))
  for move in range(80):n.send(popup,0x200,1,point(40+move*2,240+move//2));time.sleep(.003)
  n.send(popup,0x202,0,point(198,279));stop.set();sampler.join();sampler=None
  row['dragSamples']=len(samples);row['dragBackgroundColors']=sorted(set(samples))
  text(u.GetDlgItem(popup,10),'#1A73E8');time.sleep(.1)
  row['liveSwatch']=swatch(u.GetDlgItem(picture,38));row['liveUpdated']=row['liveSwatch']==0xE8731A
  text(u.GetDlgItem(popup,10),'#3278C4');time.sleep(.12)
  row['historyFrozenWhileOpen']=recent()==before
  ImageGrab.grab(bbox=n.outer(root),all_screens=True).save(out/f'{case}-live-colour.png')
  # Actual desktop click in this test process: closes popup and activates text tool.
  physical_click(u.GetDlgItem(picture,4));wait_closed(popup);popup=None;time.sleep(.15)
  row['outsideClickClosedAndReachedTool']=bool(u.IsWindowVisible(u.GetDlgItem(picture,2100)))
  row['historyFinalOnly']=recent()==['#3278C4']+[v for v in before if v!='#3278C4'][:15]
  row['closedColorKept']=swatch(u.GetDlgItem(picture,38))==0xC47832
  row['circleColorAndAlignment']=swatch(u.GetDlgItem(picture,110))==0xC47832
  first=n.outer(u.GetDlgItem(picture,110));rect=n.outer(u.GetDlgItem(picture,38))
  row['circleColorAndAlignment'] &= abs((first[1]+first[3])-(rect[1]+rect[3]))<=1
  history_snapshot=recent();popup=open_popup();text(u.GetDlgItem(popup,10),'#123');u.PostMessageW(u.GetDlgItem(popup,10),0x100,27,0);wait_closed(popup);popup=None
  row['invalidHexKeepsLast']=recent()==history_snapshot and swatch(u.GetDlgItem(picture,38))==0xC47832
  # Escape retains a valid live selection, including opacity. Same RGB is deduplicated.
  popup=open_popup();text(u.GetDlgItem(popup,11),'50');u.PostMessageW(u.GetDlgItem(popup,10),0x100,27,0);wait_closed(popup);popup=None
  row['opacityDoesNotDuplicate']=recent()==history_snapshot and value(u.GetDlgItem(picture,54)) in ('49','50')
  popup=open_popup();text(u.GetDlgItem(popup,11),'100');n.send(popup,0x201,1,point(26,176));n.send(popup,0x202,0,point(26,176));time.sleep(.08)
  row['standardIsLive']=swatch(u.GetDlgItem(picture,38))==0x3040EC
  physical_click(u.GetDlgItem(picture,4));wait_closed(popup);popup=None
  row['standardRecorded']=recent()[0]=='#EC4030'
  # Clicking an outer recent dot promotes it immediately without opening the popup.
  old=recent();n.send(u.GetDlgItem(picture,111),0xF5);time.sleep(.1)
  row['quickRecentPromoted']=recent()==[old[1],old[0]]+old[2:]
  # Dwell beyond hung-window threshold without synchronous messages.
  popup=open_popup();time.sleep(5.5)
  row['stillResponsive']=not bool(u.IsHungAppWindow(root)) and bool(u.IsWindowEnabled(root)) and bool(u.IsWindowVisible(popup))
  u.PostMessageW(u.GetDlgItem(popup,10),0x100,27,0);wait_closed(popup);popup=None
  # Equal button height, spacing and separate groups in text inspector.
  boxes=[n.outer(u.GetDlgItem(picture,i)) for i in [2101,2102,2103,2104,2110,2111]]
  row['styleButtonRects']=boxes
  row['styleLayoutConsistent']=len({(v[1],v[3],v[2]-v[0]) for v in boxes})==1 and boxes[3][0]-boxes[2][2]>=16
  ImageGrab.grab(bbox=n.outer(root),all_screens=True).save(out/f'{case}-inspector.png')
  row['passed']=all(row[k] for k in ['rootEnabled','ownedByRoot','insideRoot','noApplyCancel','liveUpdated','historyFrozenWhileOpen','outsideClickClosedAndReachedTool','historyFinalOnly','closedColorKept','circleColorAndAlignment','invalidHexKeepsLast','opacityDoesNotDuplicate','standardIsLive','standardRecorded','quickRecentPromoted','stillResponsive','styleLayoutConsistent']) and len(row['dragBackgroundColors'])==1 and row['dragSamples']>10
 expected=recent();end_process();launch();n.send(picture,d.WM_COMMAND,4)
 r['restartHistoryPreserved']=recent()==expected
 popup=open_popup();pl,pt,pr,pb=n.outer(popup);scale=(pr-pl)/292
 # The sixteenth slot is covered in native tests; select a recent color here.
 n.send(popup,0x201,1,d.packed_point(round(59*scale),round(76*scale)));n.send(popup,0x202,0,0);time.sleep(.15)
 r['popupRecentSelection']=value(u.GetDlgItem(popup,10))==expected[1]
 physical_click(u.GetDlgItem(picture,4));wait_closed(popup);popup=None
 ImageGrab.grab(bbox=n.outer(root),all_screens=True).save(out/'restart.png')
 r['passed']=all(row['passed'] for row in r['cases']) and r['restartHistoryPreserved'] and r['popupRecentSelection']
except Exception:
 import traceback;r['error']=traceback.format_exc()
finally:
 if stop:stop.set()
 if sampler:sampler.join(timeout=1)
 if popup and u.IsWindow(popup):u.PostMessageW(popup,d.WM_CLOSE,0,0)
 end_process();u.SetCursorPos(cursor.x,cursor.y)
 (out/'report.json').write_text(json.dumps(r,ensure_ascii=False,indent=2),encoding='utf-8');print(json.dumps(r,ensure_ascii=False))
raise SystemExit(0 if r['passed'] else 1)

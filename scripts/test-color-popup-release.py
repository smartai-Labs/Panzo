"""Exercise the real colour button in a disposable Panzo release process.
Never attaches to or closes an existing user process. Requires Pillow.
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
class MI(c.Structure):_fields_=[('size',w.DWORD),('monitor',w.RECT),('work',w.RECT),('flags',w.DWORD)]
mc=c.WINFUNCTYPE(w.BOOL,w.HANDLE,w.HDC,c.POINTER(w.RECT),w.LPARAM)
u.GetMonitorInfoW.argtypes=[w.HANDLE,c.POINTER(MI)];u.GetMonitorInfoW.restype=w.BOOL
monitors=[]
@mc
def monitor(h,dc,r,l):
 m=MI();m.size=c.sizeof(m);u.GetMonitorInfoW(h,c.byref(m));monitors.append({'work':[m.work.left,m.work.top,m.work.right,m.work.bottom],'primary':bool(m.flags&1)});return True
u.EnumDisplayMonitors(None,None,monitor,0)
for name,typ,ret in [('IsWindowEnabled',[w.HWND],w.BOOL),('IsHungAppWindow',[w.HWND],w.BOOL),('IsWindow',[w.HWND],w.BOOL),('InvalidateRect',[w.HWND,c.POINTER(w.RECT),w.BOOL],w.BOOL),('GetUpdateRect',[w.HWND,c.POINTER(w.RECT),w.BOOL],w.BOOL),('GetWindow',[w.HWND,w.UINT],w.HWND),('GetAncestor',[w.HWND,w.UINT],w.HWND),('SetForegroundWindow',[w.HWND],w.BOOL)]:
 f=getattr(u,name);f.argtypes=typ;f.restype=ret
profile=Path('.tmp').resolve()/('color-release-'+str(uuid.uuid4()));(profile/'Panzo').mkdir(parents=True)
fixture=profile/'Videos/Panzo/Screenshots';fixture.mkdir(parents=True)
(profile/'Panzo/settings.json').write_text(json.dumps({'theme':'dark','close_to_tray':False}),encoding='utf-8')
Image.new('RGB',(960,540),(215,222,230)).save(fixture/'color-fixture.png')
env=os.environ.copy();env.update(LOCALAPPDATA=str(profile),USERPROFILE=str(profile),APPDATA=str(profile/'Roaming'))
startup=subprocess.STARTUPINFO();startup.dwFlags|=subprocess.STARTF_USESHOWWINDOW;startup.wShowWindow=0
p=None;root=None;popup=None;stop=None;sampler=None
report={'binary':str(binary),'sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'monitors':monitors,'cases':[],'passed':False}
try:
 p=subprocess.Popen([str(binary)],env=env,startupinfo=startup);report['isolatedPid']=p.pid
 root=n.find(p,'PanzoWorkspace',20)
 # Keep each case inside a real monitor, prioritising secondary monitors.
 cases=sorted(monitors,key=lambda m:m['primary'])
 u.SetWindowPos(root,w.HWND(0),50,50,1100,800,0x40)
 n.send(root,d.WM_COMMAND,102);history=n.find(p,'PanzoCaptureHistory',20,root);n.send(history,d.WM_COMMAND,12);d.wait_for_fixture(n,p,history,20);n.send(history,d.WM_COMMAND,3)
 picture=n.find(p,'PanzoPicture',20,root);n.send(picture,d.WM_COMMAND,3)
 for i,m in enumerate(cases):
  l,t,r,b=m['work'];width=min(1100,r-l-24);height=min(800,b-t-24)
  u.SetWindowPos(root,w.HWND(0),l+12,t+12,width,height,0x40);time.sleep(.25)
  u.SetForegroundWindow(root);time.sleep(.15)
  button=u.GetDlgItem(picture,38)
  def swatch_pixel():
   dc=u.GetDC(button)
   try:return n.gdi.GetPixel(dc,17,13)
   finally:u.ReleaseDC(button,dc)
  original_swatch=swatch_pixel()
  row={'monitor':m,'root':n.outer(root),'anchor':n.outer(button)};report['cases'].append(row)
  # BM_CLICK goes through BUTTON -> BN_CLICKED -> image editor's real handler.
  assert u.PostMessageW(button,0xF5,0,0)
  popup=n.find(p,'PanzoColorPopover',10)
  time.sleep(.2);row['popup']=n.outer(popup);row['ownerMatches']=u.GetWindow(popup,4)==root;row['rootDisabled']=not bool(u.IsWindowEnabled(root))
  pl,pt,pr,pb=row['popup'];rl,rt,rr,rb=row['root']
  row['insideRoot']=rl<=pl<pr<=rr and rt<=pt<pb<=rb
  u.InvalidateRect(root,None,False)
  # Wait beyond Windows' usual ghost-window timeout, with no synchronous pings
  # that could accidentally make the old filtered message loop appear healthy.
  end=time.monotonic()+6
  while time.monotonic()<end:time.sleep(.05)
  row['visibleAfterWait']=bool(u.IsWindowVisible(popup))
  row['rootDisabledAfterWait']=not bool(u.IsWindowEnabled(root))
  row['hitPopup']=u.GetAncestor(u.WindowFromPoint(w.POINT(pl+20,pt+40)),2)==popup
  row['hung']=bool(u.IsHungAppWindow(root));row['paintPending']=bool(u.GetUpdateRect(root,None,False))
  bounds=(min(rl,pl),min(rt,pt),max(rr,pr),max(rb,pb))
  ImageGrab.grab(bbox=bounds,all_screens=True).save(out/f'case-{i}-open.png')
  # Sample an invariant pixel during real drag messages, while live preview runs.
  scale=(pr-pl)/292
  def point(x,y):return d.packed_point(round(x*scale),round(y*scale))
  samples=[];stop=threading.Event()
  def sample_background():
   dc=u.GetDC(popup)
   try:
    while not stop.is_set():samples.append(n.gdi.GetPixel(dc,round(8*scale),round(30*scale)));time.sleep(.001)
   finally:u.ReleaseDC(popup,dc)
  sampler=threading.Thread(target=sample_background);sampler.start()
  n.send(popup,0x201,1,point(40,80))
  for move in range(80):n.send(popup,0x200,1,point(40+move*2,80+move//2));time.sleep(.003)
  n.send(popup,0x202,0,point(198,119));stop.set();sampler.join()
  row['dragSamples']=len(samples);row['dragBackgroundColors']=sorted(set(samples))
  value=c.create_unicode_buffer('#1A73E8');n.send(u.GetDlgItem(popup,10),0xC,0,c.addressof(value))
  time.sleep(.15);row['liveSwatch']=swatch_pixel();row['liveUpdated']=row['liveSwatch']==0xE8731A
  u.PostMessageW(popup,d.WM_COMMAND,2,0)
  deadline=time.monotonic()+3
  while u.IsWindow(popup) and time.monotonic()<deadline:time.sleep(.02)
  row['closed']=not bool(u.IsWindow(popup));row['rootEnabledAfter']=bool(u.IsWindowEnabled(root));popup=None
  time.sleep(.15);row['cancelRestored']=swatch_pixel()==original_swatch
  # Re-open, apply a real colour, then re-open once more and verify persistence.
  assert u.PostMessageW(button,0xF5,0,0)
  popup=n.find(p,'PanzoColorPopover',5)
  hex_edit=u.GetDlgItem(popup,10)
  value=c.create_unicode_buffer('#3278C4');n.send(hex_edit,0xC,0,c.addressof(value))
  u.PostMessageW(popup,d.WM_COMMAND,1,0)
  deadline=time.monotonic()+3
  while u.IsWindow(popup) and time.monotonic()<deadline:time.sleep(.02)
  row['applyClosed']=not bool(u.IsWindow(popup));popup=None
  time.sleep(.15);row['appliedSwatch']=swatch_pixel();row['applyUpdated']=row['appliedSwatch']==0xC47832
  assert u.PostMessageW(button,0xF5,0,0)
  popup=n.find(p,'PanzoColorPopover',5)
  value=c.create_unicode_buffer(32);n.send(u.GetDlgItem(popup,10),0xD,32,c.addressof(value))
  row['reopenedHex']=value.value
  # Escape travels through the modal keyboard loop.
  u.PostMessageW(u.GetDlgItem(popup,10),0x100,0x1B,0)
  deadline=time.monotonic()+3
  while u.IsWindow(popup) and time.monotonic()<deadline:time.sleep(.02)
  row['escapeClosed']=not bool(u.IsWindow(popup));popup=None
  n.send(picture,0x201,1,d.packed_point(180,220));n.send(picture,0x200,1,d.packed_point(300,220));n.send(picture,0x202,0,d.packed_point(300,220))
  before=set(profile.rglob('*.png'));n.send(picture,d.WM_COMMAND,44)
  saved=set(profile.rglob('*.png'))-before
  row['savedStrokeHasColor']=any(sum(1 for rgb in Image.open(path).convert('RGB').getdata() if rgb==(50,120,196))>20 for path in saved)
  row['passed']=row['visibleAfterWait'] and row['rootDisabledAfterWait'] and row['hitPopup'] and row['liveUpdated'] and row['cancelRestored'] and row['applyUpdated'] and row['savedStrokeHasColor'] and row['dragSamples']>10 and len(row['dragBackgroundColors'])==1 and row['insideRoot'] and row['ownerMatches'] and row['rootDisabled'] and not row['hung'] and not row['paintPending'] and row['closed'] and row['rootEnabledAfter'] and row['applyClosed'] and row['reopenedHex']=='#3278C4' and row['escapeClosed']
 report['passed']=all(x['passed'] for x in report['cases'])
except Exception as e:report['error']=str(e)
finally:
 if stop is not None:stop.set()
 if sampler is not None:sampler.join(timeout=2)
 if p and p.poll() is None:
  if popup:u.PostMessageW(popup,d.WM_COMMAND,2,0)
  if root:u.PostMessageW(root,d.WM_CLOSE,0,0)
  try:p.wait(timeout=3)
  except subprocess.TimeoutExpired:p.kill();p.wait(timeout=5);report['cleanupForcedOwnProcess']=True
 (out/'report.json').write_text(json.dumps(report,ensure_ascii=False,indent=2),encoding='utf-8')
 print(json.dumps(report,ensure_ascii=False))
raise SystemExit(0 if report['passed'] else 1)

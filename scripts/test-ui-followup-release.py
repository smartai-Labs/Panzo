"""Native release regression. Uses an isolated profile; never closes existing Panzo processes."""
import argparse, ctypes as c, json, os, subprocess, time, uuid
from ctypes import wintypes as w
from pathlib import Path
from PIL import Image, ImageGrab
args=argparse.ArgumentParser();args.add_argument('--binary',default='target/desktop-v5/release/panzo.exe');args.add_argument('--output',default='docs/acceptance/ui-followup-20261009/native');opt=args.parse_args()
root=Path(__file__).resolve().parent.parent
out=root/opt.output;out.mkdir(parents=True,exist_ok=True)
profile=root/'.tmp'/('ui-release-'+str(uuid.uuid4()));(profile/'Panzo').mkdir(parents=True)
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
startup=subprocess.STARTUPINFO();startup.dwFlags|=subprocess.STARTF_USESHOWWINDOW;startup.wShowWindow=0
env=os.environ.copy();env['LOCALAPPDATA']=str(profile);env['USERPROFILE']=str(profile)
import shutil
fixture=profile/'fixture.panzo'
shutil.copytree(root/'.tmp/desktop-v3-review-fixtures/workspace-review-d66b9131-5476-4a73-bef4-51e9d9754678/fixture.panzo',fixture)
(fixture/'edit/recovery-draft.json').unlink(missing_ok=True)
env['RUST_BACKTRACE']='1'
error_log=(out/'process-stderr.txt').open('w',encoding='utf-8')
process=subprocess.Popen([str(root/opt.binary),str(fixture)],env=env,startupinfo=startup,stderr=error_log)
report={'passed':False,'binary':opt.binary,'checks':[]};main=None
try:
 main=find('PanzoWorkspace');wait(lambda:api.GetDlgItem(main,116));api.SetWindowPos(main,w.HWND(-1),30,30,1180,860,0x40);api.SetForegroundWindow(main);time.sleep(.6)
 r=bounds(main);tabs=[bounds(api.GetDlgItem(main,i)) for i in range(100,103)]
 assert abs((tabs[0].left+tabs[-1].right)/2-(r.left+r.right)/2)<=2
 assert bounds(api.GetDlgItem(main,111)).top-r.top<6
 report['checks'].append('menus in caption, workspace tabs geometrically centered')
 command(main,101);video=find('PanzoRecorderWindow',main,visible=True);time.sleep(1.5)
 for h,k,t in windows():
  if k=='#32770' and t=='编辑恢复':api.PostMessageW(h,0x111,1,0)
 time.sleep(.25)
 click(video,22,18);popup=find('#32768',visible=True);pr=bounds(popup);assert pr.right-pr.left>150 and pr.bottom-pr.top>120
 snapshot(main,'video-menu-dark.png');api.PostMessageW(video,0x100,27,0);api.PostMessageW(main,0x1f,0,0);time.sleep(.3)
 report['checks'].append('embedded video editor owner-drawn menu has full dimensions and visible content')
 command(main,100);panel=find('PanzoImageWorkspace',main,visible=True)
 b=api.GetDlgItem(panel,4);br=bounds(b);api.SetCursorPos(br.left+10,br.top+10)
 time.sleep(1.2)
 api.GetCursorPos.argtypes=[c.POINTER(w.POINT)];pt=w.POINT();api.GetCursorPos(c.byref(pt));api.WindowFromPoint.argtypes=[w.POINT];api.WindowFromPoint.restype=w.HWND
 report['hoverDebug']={'setCursorReturn':bool(api.SetCursorPos(br.left+10,br.top+10)),'lastError':c.get_last_error(),'button':b,'rect':[br.left,br.top,br.right,br.bottom],'cursor':[pt.x,pt.y],'windowAtCursor':api.WindowFromPoint(pt),'tooltipCounts':[(h, bool(api.IsWindowVisible(h)),message(h,0x40d)) for h,k,t in windows() if k=='tooltips_class32']}
 tips=[h for h,k,t in windows() if k=='tooltips_class32' and api.IsWindowVisible(h)]
 report['tooltip']=value(tips[0]) if tips else None;assert all(count==1 for _,_,count in report['hoverDebug']['tooltipCounts']);snapshot(main,'tooltip-dark.png');api.SetCursorPos(r.right-200,r.top+400)
 command(panel,4);collage=find('PanzoCollage',main,visible=True);assert not api.IsWindowVisible(panel)
 files=[]
 from PIL import ImageDraw
 for i,size in enumerate([(2400,1200),(1200,2400),(2700,1800),(1800,2700)]):
  im=Image.new('RGB',size,[(178,89,88),(74,132,179),(201,168,84),(125,108,162)][i]);dr=ImageDraw.Draw(im);dr.rectangle((0,0,size[0]//3,size[1]),fill=(240,230,215));dr.text((size[0]//2,size[1]//2),str(i+1),fill=(255,255,255),stroke_width=4);path=profile/f'collage-{i+1}.png';im.save(path);files.append(path)
 api.PostMessageW(collage,0x111,1,0);picker=find('#32770',title='打开',visible=True);entry=find('Edit',picker,visible=True);set_text(entry,' '.join('"'+str(p)+'"' for p in files));api.PostMessageW(picker,0x111,1,0)
 wait(lambda:not api.IsWindowVisible(picker));generate=api.GetDlgItem(collage,7);wait(lambda:api.IsWindowEnabled(generate),30)
 status=api.GetDlgItem(collage,20);assert message(api.GetDlgItem(collage,107),0x146)==4
 snapshot(main,'collage-natural-dark.png')
 edge=api.GetDlgItem(collage,104);gap=api.GetDlgItem(collage,105);set_text(combo_info(edge).edit,'1376');set_text(combo_info(gap).edit,'13')
 choose(api.GetDlgItem(collage,102),2);wait(lambda:api.IsWindowEnabled(generate),30);assert value(edge)=='1376' and value(gap)=='13';snapshot(main,'collage-cover-dark.png')
 message(gap,0x14f,1);info=combo_info(gap);wait(lambda:api.IsWindowVisible(info.list));before=message(info.list,0x18e)
 for _ in range(4):message(info.list,0x20a,(0xff88<<16),0)
 after=message(info.list,0x18e);assert after>before,(before,after);snapshot(main,'dropdown-scrolled-dark.png');message(gap,0x14f,0)
 report['checks'].append('editable numeric combos retain custom values; overflowing owner-drawn dropdown scrolls with wheel')
 choose(api.GetDlgItem(collage,107),0);first=value(api.GetDlgItem(collage,107));command(collage,3);wait(lambda:api.IsWindowEnabled(generate),30);assert message(api.GetDlgItem(collage,107),0x147)==1
 command(main,101);command(main,100);assert api.IsWindowVisible(collage) and value(edge)=='1376'
 report['checks'].append('collage imports four photos, previews natural/cover modes, reorders one photo, preserves draft across tabs')
 # Cropping is directly manipulated in the image preview; choose square cells and start inside the second cell.
 preview=find('PanzoAssetPreview',collage,visible=True);rr=bounds(preview);cx=(rr.right-rr.left)*3//4;cy=(rr.bottom-rr.top)//3
 api.PostMessageW(preview,0x201,1,(cy<<16)|cx);api.PostMessageW(preview,0x200,1,((cy+20)<<16)|(cx+30));api.PostMessageW(preview,0x202,0,((cy+20)<<16)|(cx+30));time.sleep(.4)
 wait(lambda:api.IsWindowEnabled(generate),30);assert value(api.GetDlgItem(collage,109))!='50' or value(api.GetDlgItem(collage,110))!='50';snapshot(main,'collage-crop-adjusted.png')
 command(collage,7);picture=find('PanzoPicture',main,visible=True);assert not api.IsWindowVisible(collage)
 thumb=api.GetDlgItem(panel,101);wait(lambda:'1376' in value(thumb));report['checks'].append('generate creates full-resolution editable image and returns to image workspace')
 # Exercise tooltip and custom stroke/font values, independent from presets.
 command(picture,32);stroke=api.GetDlgItem(picture,50);set_text(combo_info(stroke).edit,'7');assert value(stroke)=='7'
 command(picture,4);font=api.GetDlgItem(picture,51);set_text(combo_info(font).edit,'36');assert value(font)=='36';snapshot(main,'font-size-dark.png')
 report['checks'].append('stroke 7 and font 36 accept manually typed non-preset values')
 # Resize in small increments while inspecting fixed caption text pixels. All sampled pixels belong to this test process.
 import threading
 api.SetWindowPos(main,w.HWND(-1),30,30,1180,860,0x40);time.sleep(.4);resize_anchor=bounds(main);original=bounds(api.GetDlgItem(main,111));patch=ImageGrab.grab(bbox=(original.left,original.top,original.right,original.bottom)).convert('RGB')
 points=[(x+original.left,y+original.top) for y in range(8,patch.height-5) for x in range(8,patch.width-5) if min(patch.getpixel((x,y)))>160][::4][:16]
 assert points
 gdi=c.WinDLL('gdi32');api.GetDC.argtypes=[w.HWND];api.GetDC.restype=w.HDC;api.ReleaseDC.argtypes=[w.HWND,w.HDC];gdi.GetPixel.argtypes=[w.HDC,c.c_int,c.c_int];gdi.GetPixel.restype=w.DWORD
 failures=[];samples=0
 def resizing():
  for i in range(100):
   width=1180+(i%20-10)*5;height=860+(i%16-8)*3;api.SetWindowPos(main,None,resize_anchor.left,resize_anchor.top,width,height,0x14);time.sleep(.025)
 worker=threading.Thread(target=resizing);worker.start();dc=api.GetDC(None)
 try:
  while worker.is_alive():
   values=[gdi.GetPixel(dc,x,y) for x,y in points];bright=sum(min(v&255,(v>>8)&255,(v>>16)&255)>130 for v in values);samples+=1
   if bright<len(points)//2:failures.append({'sample':samples,'bright':bright})
 finally:api.ReleaseDC(None,dc);worker.join()
 api.SetWindowPos(main,w.HWND(-1),30,30,1180,860,0x40);report['resize']={'resizeSteps':100,'measurement':'screen pixel samples, not display frames','samples':samples,'missingCaptionTextFrames':len(failures),'firstFailures':failures[:8]}
 assert not failures,report['resize'];report['checks'].append('100 resize steps preserve caption text without erase frames')
 command(main,110);snapshot(main,'workspace-light.png')
 api.PostMessageW(main,0x10,0,0);process.wait(timeout=10);assert process.returncode==0;report['passed']=True
finally:
 if not report['passed']:
  report['dialogs']=[{'window':(h,k,t),'children':windows(h)} for h,k,t in windows() if k=='#32770']
  if main and process.poll() is None:snapshot(main,'failure.png')
 if process.poll() is None:
  if main:api.PostMessageW(main,0x1f,0,0);api.PostMessageW(main,0x10,0,0)
  try:process.wait(timeout=3)
  except subprocess.TimeoutExpired:process.kill();process.wait()
 error_log.close()
 (out/'followup-smoke.json').write_text(json.dumps(report,ensure_ascii=False,indent=2),encoding='utf-8')
print(json.dumps(report,ensure_ascii=False))



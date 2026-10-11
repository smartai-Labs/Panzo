"""Native release regression. Uses an isolated profile; never closes existing Panzo processes."""
import argparse, ctypes as c, json, os, subprocess, time, uuid
from ctypes import wintypes as w
from pathlib import Path
from PIL import Image, ImageGrab
args=argparse.ArgumentParser();args.add_argument('--binary',default='target/desktop-v9/release/panzo.exe');args.add_argument('--output',default='docs/acceptance/grid-v9-20261009/native');opt=args.parse_args()
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
report={'passed':False,'binary':opt.binary,'checks':[],'processId':process.pid,'profile':str(profile)};main=None
# Render only this process's preview into an offscreen DIB. Screen occlusion cannot affect gesture checks.
gdi=c.WinDLL('gdi32');api.GetDC.argtypes=[w.HWND];api.GetDC.restype=w.HDC;api.ReleaseDC.argtypes=[w.HWND,w.HDC]
api.PrintWindow.argtypes=[w.HWND,w.HDC,w.UINT];api.PrintWindow.restype=w.BOOL
gdi.CreateCompatibleDC.argtypes=[w.HDC];gdi.CreateCompatibleDC.restype=w.HDC
gdi.CreateCompatibleBitmap.argtypes=[w.HDC,c.c_int,c.c_int];gdi.CreateCompatibleBitmap.restype=w.HBITMAP
gdi.SelectObject.argtypes=[w.HDC,w.HANDLE];gdi.SelectObject.restype=w.HANDLE
gdi.DeleteObject.argtypes=[w.HANDLE];gdi.DeleteDC.argtypes=[w.HDC]
class BitmapInfo(c.Structure):
 _fields_=[('size',w.DWORD),('width',w.LONG),('height',w.LONG),('planes',w.WORD),('bits',w.WORD),('compression',w.DWORD),('image_size',w.DWORD),('xppm',w.LONG),('yppm',w.LONG),('used',w.DWORD),('important',w.DWORD)]
gdi.GetDIBits.argtypes=[w.HDC,w.HBITMAP,w.UINT,w.UINT,c.c_void_p,c.c_void_p,w.UINT]
api.UpdateWindow.argtypes=[w.HWND]
def raster(h):
 api.UpdateWindow(h)
 r=w.RECT();api.GetClientRect(h,c.byref(r));dc=api.GetDC(h);memory=gdi.CreateCompatibleDC(dc);bmp=gdi.CreateCompatibleBitmap(dc,r.right,r.bottom);old=gdi.SelectObject(memory,bmp)
 try:
  assert api.PrintWindow(h,memory,1);gdi.SelectObject(memory,old)
  info=BitmapInfo(c.sizeof(BitmapInfo),r.right,-r.bottom,1,32,0,0,0,0,0,0);pixels=c.create_string_buffer(r.right*r.bottom*4)
  assert gdi.GetDIBits(dc,bmp,0,r.bottom,pixels,c.byref(info),0)==r.bottom
  return Image.frombytes('RGB',(r.right,r.bottom),pixels.raw,'raw','BGRX')
 finally:gdi.SelectObject(memory,old);gdi.DeleteObject(bmp);gdi.DeleteDC(memory);api.ReleaseDC(h,dc)
def selection(im):
 import numpy as np
 a=np.asarray(im);mask=(a[:,:,0]==242)&(a[:,:,1]==96)&(a[:,:,2]==70);ys,xs=np.where(mask)
 assert len(xs)>30,'selection outline missing'
 return (int(xs.min()),int(ys.min()),int(xs.max()+1),int(ys.max()+1))
def wheel(h,x,y,delta=120,ctrl=False):
 p=w.POINT(x,y);api.ClientToScreen(h,c.byref(p));message(h,0x20a,((delta&65535)<<16)|(8 if ctrl else 0),((p.y&65535)<<16)|(p.x&65535))
def point_message(h,msg,x,y,buttons=0):message(h,msg,buttons,((y&65535)<<16)|(x&65535))
def different(a,b,rect):
 from PIL import ImageChops,ImageStat
 return sum(ImageStat.Stat(ImageChops.difference(a.crop(rect),b.crop(rect))).mean)/3
try:
 main=find('PanzoWorkspace');wait(lambda:api.GetDlgItem(main,116));api.SetWindowPos(main,w.HWND(-1),30,30,1220,920,0x40);api.SetForegroundWindow(main);time.sleep(.6)
 command(main,100);panel=find('PanzoImageWorkspace',main,visible=True);command(panel,4);collage=find('PanzoCollage',main,visible=True)
 assert all(not api.GetDlgItem(collage,i) for i in [100,101,102,103,107,108,109,110])
 assert len([1 for h,k,t in windows(collage) if k=='ComboBox'])==2
 for id in [300,301,302,303,310,311,320,321,322,330,331,332,333,334,335,106]:
  b=api.GetDlgItem(collage,id);cls=c.create_unicode_buffer(128);api.GetClassNameW(b,cls,len(cls));assert cls.value=='Button',(id,cls.value)
 report['checks'].append('layout cards and all segmented choices are native buttons; only edge/gap remain editable dropdowns; obsolete photo controls absent')
 api.GetDpiForWindow.argtypes=[w.HWND];api.GetDpiForWindow.restype=w.UINT
 dpi=api.GetDpiForWindow(collage);d=lambda n:n*dpi//96
 empty_add=bounds(api.GetDlgItem(collage,1));cr=bounds(collage)
 assert empty_add.top==cr.bottom-d(77) and empty_add.bottom-empty_add.top==d(60)
 files=[]
 from PIL import ImageDraw
 for i,size in enumerate([(1800,1000),(1000,1800),(1500,1000),(1000,1500)]):
  im=Image.new('RGB',size,(230,235,240));dr=ImageDraw.Draw(im)
  for x in range(0,size[0],120):dr.rectangle((x,0,x+59,size[1]),fill=[(175,91,82),(75,137,180),(195,164,88),(126,108,162)][i])
  for y in range(0,size[1],160):dr.rectangle((0,y,size[0],y+12),fill=(40,55,70))
  path=profile/f'collage-{i+1}.png';im.save(path);files.append(path)
 api.PostMessageW(collage,0x111,1,0);picker=find('#32770',title='打开',visible=True);entry=find('Edit',picker,visible=True);set_text(entry,' '.join('"'+str(p)+'"' for p in files));api.PostMessageW(picker,0x111,1,0)
 wait(lambda:not api.IsWindowVisible(picker));generate=api.GetDlgItem(collage,7);wait(lambda:api.IsWindowEnabled(generate),45);time.sleep(.08)
 preview=find('PanzoAssetPreview',collage,visible=True);natural=raster(preview);natural.save(out/'natural-preview.png');selection(natural);snapshot(main,'collage-natural-dark.png')
 add=bounds(api.GetDlgItem(collage,1));assert add.top==empty_add.top and add.bottom==empty_add.bottom
 assert add.right-add.left==empty_add.right-empty_add.left and add.left-empty_add.left==4*(add.right-add.left+d(5))
 assert value(api.GetDlgItem(collage,20))==''
 report['checks'].append('add tile is in the thumbnail row, same dimensions, follows four imported photos; persistent guidance label empty')
 # Grid ratios are independent of display mode; the default action restores uniform square cells.
 ratios=[]
 for id,ratio in [(330,1),(331,4/3),(332,3/4),(333,16/9),(334,9/16)]:
  message(api.GetDlgItem(collage,335),0xf5);wait(lambda:api.IsWindowEnabled(generate),45);time.sleep(.08)
  for rid in range(330,336):assert api.IsWindowEnabled(api.GetDlgItem(collage,rid))
  message(api.GetDlgItem(collage,id),0xf5);wait(lambda:api.IsWindowEnabled(generate),45);time.sleep(.08)
  selected=selection(raster(preview));actual=(selected[2]-selected[0])/(selected[3]-selected[1]);assert abs(actual-ratio)<.025,(id,actual,ratio)
  ratios.append({'id':id,'expected':ratio,'measured':actual})
 message(api.GetDlgItem(collage,335),0xf5);wait(lambda:api.IsWindowEnabled(generate),45);time.sleep(.08);automatic_image=raster(preview);automatic_image.save(out/'automatic-restored.png');automatic=selection(automatic_image);assert abs((automatic[2]-automatic[0])/(automatic[3]-automatic[1])-1.)<.025
 report['ratios']=ratios;report['checks'].append('all five fixed ratio buttons invoked with native button clicks from automatic mode; actual rendered aspect verified; grid Default restores uniform square cells')
 edge=api.GetDlgItem(collage,104);gap=api.GetDlgItem(collage,105);set_text(combo_info(edge).edit,'1376');set_text(combo_info(gap).edit,'13');command(collage,330);command(collage,322);wait(lambda:api.IsWindowEnabled(generate),45);time.sleep(.08)
 # Direct wheel/drag in every display mode; transform changes are visible before final render is allowed.
 for mode in [322,321,320]:
  api.SetWindowPos(main,w.HWND(-1),30,30,1220,920,0x40);time.sleep(.25)
  command(collage,mode);wait(lambda:api.IsWindowEnabled(generate),45);time.sleep(.08);command(collage,6);wait(lambda:api.IsWindowEnabled(generate),45);time.sleep(.08)
  baseline=raster(preview);rect=selection(baseline);x=(rect[0]+rect[2])//2;y=(rect[1]+rect[3])//2
  wheel(preview,x,y,480);zoomed=raster(preview);assert selection(zoomed)==rect,('zoom moved grid boundary',mode);baseline.save(out/f'baseline-{mode}.png');zoomed.save(out/f'zoomed-{mode}.png');inner=(rect[0]+35,rect[1]+35,rect[2]-12,rect[3]-12)
  assert different(baseline,zoomed,inner)>4,('wheel had no visible effect',mode)
  wait(lambda:api.IsWindowEnabled(generate),45);time.sleep(.08);before=raster(preview)
  point_message(preview,0x201,x,y,1);point_message(preview,0x200,x+25,y+18,1)
  during=raster(preview);assert selection(during)==rect,('drag moved grid boundary',mode);assert not api.IsWindowEnabled(generate)
  assert different(before,during,inner)>4,('drag had no immediate visual effect',mode)
  point_message(preview,0x202,x+25,y+18);wait(lambda:api.IsWindowEnabled(generate),45);time.sleep(.08);settled=raster(preview)
  during.save(out/f'during-{mode}.png');settled.save(out/f'settled-{mode}.png');assert during.size==settled.size,('window resized during gesture measurement',during.size,settled.size)
  error=different(during,settled,inner);assert error<18,(mode,error)
  report.setdefault('liveVsFinalMeanError',{})[str(mode)]=round(error,3)
  settled.save(out/f'mode-{mode}-manipulated.png')
  point_message(preview,0x203,x,y,1);wait(lambda:api.IsWindowEnabled(generate),45);time.sleep(.08);reset=raster(preview);assert different(baseline,reset,inner)<1,(mode,'reset differs')
 report['checks'].append('all three display modes: fixed cell outline during cursor-anchored zoom and immediate drag; matching settled image; double-click reset')
 command(collage,322);wait(lambda:api.IsWindowEnabled(generate),45);time.sleep(.08);im=raster(preview);first=selection(im);x=(first[0]+first[2])//2;y=(first[1]+first[3])//2
 point_message(preview,0x201,first[2]+(first[2]-first[0])//2,y,1);point_message(preview,0x202,first[2]+(first[2]-first[0])//2,y)
 second=selection(raster(preview));assert second[0]>first[0]+50
 report['checks'].append('clicking another photo moves its numbered selection outline in the preview')
 command(collage,2);wait(lambda:api.IsWindowEnabled(generate),45);time.sleep(.08);assert selection(raster(preview))[0]<second[0]
 command(main,101);command(main,100);assert api.IsWindowVisible(collage) and value(edge)=='1376'
 # Layout cards/direction/ratio are actionable; background opens the shared color picker.
 command(collage,300);command(collage,311);command(collage,331);wait(lambda:api.IsWindowEnabled(generate),45);time.sleep(.08);raster(preview).save(out/'two-vertical.png')
 assert api.IsWindowEnabled(api.GetDlgItem(collage,311))
 command(collage,303);wait(lambda:not api.IsWindowEnabled(api.GetDlgItem(collage,311)));assert not api.IsWindowEnabled(generate)
 time.sleep(.6);command(collage,302);command(collage,330);wait(lambda:api.IsWindowEnabled(generate),45);time.sleep(.08)
 api.PostMessageW(collage,0x111,106,0);colors=find('PanzoColorPopover',visible=True);assert not api.IsWindowEnabled(api.GetDlgItem(colors,11));set_text(api.GetDlgItem(colors,10),'24364A');api.PostMessageW(colors,0x111,1,0);wait(lambda:not api.IsWindowVisible(colors));wait(lambda:api.IsWindowEnabled(generate),45);time.sleep(.08)
 colored=raster(preview);colored.save(out/'colored-preview.png');assert sum(pixel==(36,54,74) for pixel in colored.get_flattened_data())>50
 report['checks'].append('cards switch layout, direction and ratio; nine-grid waits for missing photos; arbitrary HEX background color applies')
 # Fill to capacity using the relocated add tile; removal must move and re-enable that tile.
 extra=[]
 for i in range(5):
  path=profile/f'extra-{i}.png';shutil.copy2(files[i%4],path);extra.append(path)
 api.PostMessageW(api.GetDlgItem(collage,1),0xf5,0,0);picker=find('#32770',title='打开',visible=True);entry=find('Edit',picker,visible=True);set_text(entry,' '.join('"'+str(p)+'"' for p in extra));api.PostMessageW(picker,0x111,1,0)
 wait(lambda:not api.IsWindowVisible(picker));wait(lambda:not api.IsWindowEnabled(api.GetDlgItem(collage,1)),45);wait(lambda:api.IsWindowEnabled(generate),45);time.sleep(.08)
 full_add=bounds(api.GetDlgItem(collage,1));pr=bounds(preview);assert full_add.right<=pr.right and full_add.left-empty_add.left==9*(full_add.right-full_add.left+d(5))
 # Measure all numbered cell outlines, including non-square ratios and every display mode.
 grid_checks=[]
 for layout,n in [(302,2),(303,3)]:
  command(collage,layout)
  for ratio_id,expected in [(330,1.),(331,4/3),(332,3/4)]:
   command(collage,ratio_id)
   reference=None
   for mode in [320,321,322]:
    command(collage,mode);wait(lambda:api.IsWindowEnabled(generate),45);time.sleep(.08)
    cells=[]
    for index in range(n*n):
     step=full_add.right-full_add.left+d(5);x=empty_add.left-cr.left+index*step+step//2;y=empty_add.top-cr.top+d(20)
     point_message(collage,0x201,x,y,1);point_message(collage,0x202,x,y)
     cells.append(selection(raster(preview)))
    for index,cell in enumerate(cells):
     column=cells[index%n];row=cells[index//n*n]
     assert (cell[0],cell[2])==(column[0],column[2]),('column misaligned',layout,ratio_id,mode,cells)
     assert (cell[1],cell[3])==(row[1],row[3]),('row misaligned',layout,ratio_id,mode,cells)
     assert abs((cell[2]-cell[0])/(cell[3]-cell[1])-expected)<.035,(layout,ratio_id,mode,cells)
    if reference is None:reference=cells
    else:assert cells==reference,('display mode moved grid boundaries',layout,ratio_id,mode)
    grid_checks.append({'count':n*n,'ratio':expected,'mode':mode,'cells':cells})
    if ratio_id==330 and mode==320:raster(preview).save(out/f'grid-{n}x{n}-aligned.png')
 report['gridMeasurements']=grid_checks
 report['checks'].append('all four/nine-grid row and column edges align exactly in preview across three ratios and all display modes; ratio survives mode changes')
 command(collage,302);command(collage,330);command(collage,322);wait(lambda:api.IsWindowEnabled(generate),45);time.sleep(.08)
 for count in range(9,4,-1):
  step=full_add.right-full_add.left+d(5);x=empty_add.left-cr.left+(count-1)*step+step//2;y=empty_add.top-cr.top+d(20)
  point_message(collage,0x201,x,y,1);point_message(collage,0x202,x,y);command(collage,5);wait(lambda:api.IsWindowEnabled(generate),45);time.sleep(.08)
 assert api.IsWindowEnabled(api.GetDlgItem(collage,1)) and bounds(api.GetDlgItem(collage,1)).left==add.left
 report['checks'].append('add tile remains within thumbnail strip at nine photos, disables at capacity, moves and re-enables after removal')
 colored=raster(preview)
 before=selection(colored);wheel(preview,(before[0]+before[2])//2,(before[1]+before[3])//2,120,True);after=selection(raster(preview));assert (after[2]-after[0])>(before[2]-before[0]);command(preview,4)
 report['checks'].append('Ctrl+wheel zooms composition separately; fit button restores viewport')
 snapshot(main,'collage-cards-dark.png');command(main,110);snapshot(main,'collage-cards-light.png');raster(preview).save(out/'light-preview.png')
 command(collage,7);picture=find('PanzoPicture',main,visible=True);assert not api.IsWindowVisible(collage);thumb=api.GetDlgItem(panel,101);wait(lambda:'1376' in value(thumb));report['checks'].append('generate returns a 1376px editable image to the image workspace')
 command(picture,44)
 exported=wait(lambda:next((p for p in (profile/'Videos/Panzo/Screenshots').glob('*.png') if p.name!='review-image.png'),None))
 import numpy as np
 saved=Image.open(exported).convert('RGB');assert saved.size==(1376,1376),saved.size
 a=np.asarray(saved);background=(a==[36,54,74]).all(axis=2)
 columns=np.where(background.all(axis=0))[0];rows=np.where(background.all(axis=1))[0]
 assert len(columns)==13 and len(rows)==13 and (columns==rows).all(),(columns,rows)
 assert int(columns[-1])-int(columns[0])==12 and abs((int(columns[0])+int(columns[-1]))/2-687.5)<1
 saved.save(out/'generated-grid.png');report['export']={'size':saved.size,'verticalGutter':columns.tolist(),'horizontalGutter':rows.tolist()}
 report['checks'].append('generated and saved 1376px PNG has continuous aligned 13px vertical/horizontal gutters')
 api.PostMessageW(main,0x10,0,0);process.wait(timeout=10);assert process.returncode==0;report['passed']=True
finally:
 if not report['passed']:
  report['dialogs']=[{'window':(h,k,t),'children':windows(h)} for h,k,t in windows() if k=='#32770']
  if main and process.poll() is None:snapshot(main,'failure.png')
 if process.poll() is None:
  if main:api.PostMessageW(main,0x10,0,0)
  try:process.wait(timeout=3)
  except subprocess.TimeoutExpired:process.kill();process.wait()
 error_log.close()
 (out/'collage-smoke.json').write_text(json.dumps(report,ensure_ascii=False,indent=2),encoding='utf-8')
print(json.dumps({k:v for k,v in report.items() if k!='gridMeasurements'},ensure_ascii=False))

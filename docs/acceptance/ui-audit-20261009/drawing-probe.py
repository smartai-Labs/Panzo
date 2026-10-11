"""Audit drawing of a synthetic image in an isolated release process; no production source changes."""
import ctypes as c
from ctypes import wintypes as w
import json, os, subprocess, time, threading, uuid
from pathlib import Path
from PIL import Image, ImageGrab
base=Path.cwd()
out=base/'docs/acceptance/ui-audit-20261009'
out.mkdir(parents=True,exist_ok=True)
profile=base/'.tmp'/('ui-audit-'+str(uuid.uuid4()))
(profile/'Panzo').mkdir(parents=True)
(profile/'Videos/Panzo/Screenshots').mkdir(parents=True)
(profile/'Panzo/settings.json').write_text(json.dumps({'theme':'dark','close_to_tray':False}),encoding='utf-8')
fixture=profile/'Videos/Panzo/Screenshots/synthetic-drawing.png'
Image.new('RGB',(4000,2500),(232,234,238)).save(fixture)
api=c.WinDLL('user32',use_last_error=True)
gdi=c.WinDLL('gdi32',use_last_error=True)
api.GetWindowThreadProcessId.argtypes=[w.HWND,c.POINTER(w.DWORD)]
api.GetClassNameW.argtypes=[w.HWND,w.LPWSTR,c.c_int]
api.GetDlgItem.argtypes=[w.HWND,c.c_int];api.GetDlgItem.restype=w.HWND
api.GetWindowRect.argtypes=[w.HWND,c.POINTER(w.RECT)]
api.GetClientRect.argtypes=[w.HWND,c.POINTER(w.RECT)]
api.ClientToScreen.argtypes=[w.HWND,c.POINTER(w.POINT)]
api.SetWindowPos.argtypes=[w.HWND,w.HWND,c.c_int,c.c_int,c.c_int,c.c_int,w.UINT]
api.GetDC.argtypes=[w.HWND];api.GetDC.restype=w.HDC
api.ReleaseDC.argtypes=[w.HWND,w.HDC]
gdi.GetPixel.argtypes=[w.HDC,c.c_int,c.c_int];gdi.GetPixel.restype=w.DWORD
api.SendMessageTimeoutW.argtypes=[w.HWND,w.UINT,c.c_size_t,c.c_ssize_t,w.UINT,w.UINT,c.POINTER(c.c_size_t)]
api.SendMessageTimeoutW.restype=c.c_size_t
api.PostMessageW.argtypes=[w.HWND,w.UINT,c.c_size_t,c.c_ssize_t]
callback=c.WINFUNCTYPE(w.BOOL,w.HWND,w.LPARAM)
api.EnumWindows.argtypes=[callback,w.LPARAM]
api.EnumChildWindows.argtypes=[w.HWND,callback,w.LPARAM]
def windows(pid,parent=None):
    values=[]
    @callback
    def each(hwnd,_):
        owner=w.DWORD();api.GetWindowThreadProcessId(hwnd,c.byref(owner))
        if owner.value==pid:
            name=c.create_unicode_buffer(128);api.GetClassNameW(hwnd,name,len(name))
            values.append((hwnd,name.value))
        return True
    if parent:api.EnumChildWindows(parent,each,0)
    else:api.EnumWindows(each,0)
    return values
def send(hwnd,msg,wp=0,lp=0):
    value=c.c_size_t()
    if not api.SendMessageTimeoutW(hwnd,msg,wp,lp,2,5000,c.byref(value)):
        raise RuntimeError('Message timed out')
    return value.value
def find(kind,parent=None):
    deadline=time.monotonic()+12
    while time.monotonic()<deadline:
        found=[hwnd for hwnd,name in windows(process.pid,parent) if name==kind]
        if found:return found[0]
        if process.poll() is not None:raise RuntimeError('Test app exited')
        time.sleep(.05)
    raise RuntimeError('Window not found: '+kind)
def xy(x,y):return (int(y)<<16)|(int(x)&0xffff)
startup=subprocess.STARTUPINFO();startup.dwFlags|=subprocess.STARTF_USESHOWWINDOW;startup.wShowWindow=0
env=os.environ.copy();env['LOCALAPPDATA']=str(profile);env['USERPROFILE']=str(profile)
process=subprocess.Popen([str(base/'target/desktop-v3/release/panzo.exe')],env=env,startupinfo=startup)
report={'test':'synthetic image drawing sampling','imageSize':[4000,2500],'sampled':False,'uniqueProfile':str(profile)}
hwnd=None
try:
    hwnd=find('PanzoWorkspace')
    api.SetWindowPos(hwnd,w.HWND(-1),50,50,1100,800,0x40)
    send(hwnd,0x111,102)
    history=find('PanzoCaptureHistory',hwnd)
    send(history,0x111,12)
    send(history,0x111,3)
    picture=find('PanzoPicture',hwnd)
    time.sleep(.5)
    rect=w.RECT();api.GetClientRect(picture,c.byref(rect))
    origin=w.POINT();api.ClientToScreen(picture,c.byref(origin))
    outer=w.RECT();api.GetWindowRect(hwnd,c.byref(outer))
    ImageGrab.grab(bbox=(outer.left,outer.top,outer.right,outer.bottom)).save(out/'drawing-before.png')
    dc=api.GetDC(None)
    point=(origin.x+rect.right//2,origin.y+rect.bottom//3)
    initial=gdi.GetPixel(dc,*point)
    expected=232|(234<<8)|(238<<16)
    if initial!=expected:raise RuntimeError('Probe is not on the unchanged synthetic image: '+hex(initial))
    send(picture,0x111,3)
    samples=[]
    def sample(duration):
        start=time.perf_counter()
        while time.perf_counter()-start<duration:
            samples.append((round((time.perf_counter()-start)*1000,3),int(gdi.GetPixel(dc,*point))))
            time.sleep(.002)
    sampling=threading.Thread(target=sample,args=(2.8,))
    sampling.start()
    time.sleep(.2)
    x1=rect.right//3;y=rect.bottom*2//3
    send(picture,0x201,1,xy(x1,y))
    for index in range(90):
        send(picture,0x200,1,xy(x1+index*2,y+(index%12)))
        time.sleep(.012)
    send(picture,0x202,0,xy(x1+178,y+5))
    sampling.join()
    api.ReleaseDC(None,dc)
    time.sleep(.2)
    ImageGrab.grab(bbox=(outer.left,outer.top,outer.right,outer.bottom)).save(out/'drawing-after.png')
    changed=[row for row in samples if row[1]!=expected]
    report.update(sampled=True,unchangedPixel=point,expectedColorRef=expected,samples=len(samples),
                  changedSamples=len(changed),observedColors=sorted(set(row[1] for row in samples)),
                  firstChanges=changed[:20],samplingMilliseconds=samples[-1][0],
                  limit='Pixel sampling only; counts are not display frame counts. A zero count cannot exclude shorter flashes.')
    (out/'drawing-samples.json').write_text(json.dumps(samples),encoding='utf-8')
finally:
    if process.poll() is None:
        if hwnd:api.PostMessageW(hwnd,0x10,0,0)
        try:process.wait(timeout=4)
        except subprocess.TimeoutExpired:process.kill();process.wait()
    (out/'drawing-audit.json').write_text(json.dumps(report,ensure_ascii=False,indent=2),encoding='utf-8')
print(json.dumps(report,ensure_ascii=False))


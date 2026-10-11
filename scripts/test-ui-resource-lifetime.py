#!/usr/bin/env python3
"""Bounded GDI/window lifetime and warm idle sample in an isolated, self-owned host."""
import argparse, ctypes as c, importlib.util, json, os, subprocess, time, uuid, hashlib
from pathlib import Path
from ctypes import wintypes as w

def main():
    parser=argparse.ArgumentParser();parser.add_argument('--binary',type=Path,required=True);parser.add_argument('--output',type=Path,required=True);args=parser.parse_args()
    repo=Path(__file__).resolve().parent.parent;out=args.output.resolve();out.mkdir(parents=True,exist_ok=True)
    spec=importlib.util.spec_from_file_location('probe',repo/'scripts/test-drawing-stability.py');helper=importlib.util.module_from_spec(spec);spec.loader.exec_module(helper);n=helper.Native()
    n.api.RedrawWindow.argtypes=[w.HWND,c.c_void_p,w.HANDLE,w.UINT];n.api.RedrawWindow.restype=w.BOOL
    n.api.GetGuiResources.argtypes=[w.HANDLE,w.DWORD];n.api.GetGuiResources.restype=w.DWORD
    kernel=c.WinDLL('kernel32');kernel.GetProcessTimes.argtypes=[w.HANDLE]+[c.POINTER(w.FILETIME)]*4;kernel.GetProcessTimes.restype=w.BOOL
    profile=repo/'.tmp'/('ui-lifetime-'+str(uuid.uuid4()));(profile/'Panzo').mkdir(parents=True)
    (profile/'Panzo/settings.json').write_text(json.dumps({'theme':'dark','close_to_tray':False,'animations':False}),encoding='utf-8')
    env=os.environ.copy();env.update(LOCALAPPDATA=str(profile),USERPROFILE=str(profile))
    dump_dir=Path(os.environ['LOCALAPPDATA'])/'CrashDumps'
    dumps_before={f.name:f.stat().st_mtime_ns for f in dump_dir.glob('panzo*.dmp')}
    p=subprocess.Popen([str(args.binary.resolve())],env=env,cwd=repo)
    report={'passed':False,'binarySha256':hashlib.sha256(args.binary.read_bytes()).hexdigest(),'pid':p.pid,'profile':str(profile),'cycles':[]};root=None
    def cpu():
        stamps=[w.FILETIME() for _ in range(4)];assert kernel.GetProcessTimes(w.HANDLE(int(p._handle)),*[c.byref(v) for v in stamps]);return sum((v.dwHighDateTime<<32)|v.dwLowDateTime for v in stamps[2:])/1e7
    def resources():return {'gdi':n.api.GetGuiResources(w.HANDLE(int(p._handle)),0),'user':n.api.GetGuiResources(w.HANDLE(int(p._handle)),1)}
    try:
        root=n.find(p,'PanzoWorkspace',30)
        for index in range(110):
            n.send(root,0x111,116 if index%2==0 else 115)
            deadline=time.monotonic()+5
            visible=[]
            while not visible and time.monotonic()<deadline:
                visible=[h for h,k in n.windows(p.pid) if k=='PanzoManagementWindow' and n.api.IsWindowVisible(h)]
                if not visible:time.sleep(.01)
            assert visible,'management window never became visible'
            for h in visible:
                n.api.RedrawWindow(h,None,None,0x180)
                n.send(h,0x10)
            time.sleep(.015)
            if index==9:report['baseline']=resources()
            if index>=10:report['cycles'].append(resources())
        report['final']=resources();assert report['final']['gdi']-report['baseline']['gdi']<=8,report
        assert report['final']['user']-report['baseline']['user']<=4,report
        before=cpu();start=time.perf_counter();time.sleep(30);elapsed=time.perf_counter()-start
        report['idle']={'seconds':elapsed,'oneCorePercent':(cpu()-before)/elapsed*100,'note':'warm empty workspace, no recording, playback or background media preparation'}
        n.api.PostMessageW(root,0x10,0,0);p.wait(timeout=10);report['exitCode']=p.returncode;assert p.returncode==0
        time.sleep(2)
        report['newCrashDumps']=[f.name for f in dump_dir.glob(f'panzo.exe.{p.pid}.dmp') if dumps_before.get(f.name)!=f.stat().st_mtime_ns]
        assert not report['newCrashDumps'],report['newCrashDumps']
        report['passed']=True
    finally:
        if p.poll() is None:
            if root:n.api.PostMessageW(root,0x10,0,0)
            try:p.wait(timeout=4)
            except subprocess.TimeoutExpired:p.kill();p.wait()
        (out/'resources.json').write_text(json.dumps(report,ensure_ascii=False,indent=2),encoding='utf-8')
    print(json.dumps({k:v for k,v in report.items() if k!='cycles'},ensure_ascii=False))
if __name__=='__main__':main()

#!/usr/bin/env python3
"""Read-only release UI timing probes in newly launched isolated Panzo processes.
Measurements are Win32 API/message round trips, NOT presentation latency or FPS.
Never attaches to an existing process. Uses synthetic image and an optional COPY
of a supplied test project. Profiles and raw timings are retained for review.
"""
import argparse, ctypes as c, hashlib, importlib.util, json, math, os
from pathlib import Path
import shutil, statistics, subprocess, time, uuid
from ctypes import wintypes as w
from PIL import Image, ImageGrab


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--fixture-project', type=Path)
    parser.add_argument('--phase', choices=['image','video','both'], default='both')
    parser.add_argument('--include-cache', action='store_true')
    parser.add_argument('--flush-paint', action='store_true', help='Include pending child paint completion in each resize probe')
    args=parser.parse_args()
    repository=Path(__file__).resolve().parent.parent
    binary=args.binary.resolve(); out=args.output.resolve(); out.mkdir(parents=True,exist_ok=True)
    spec=importlib.util.spec_from_file_location('panzo_drawing_probe', repository/'scripts/test-drawing-stability.py')
    helper=importlib.util.module_from_spec(spec); spec.loader.exec_module(helper)
    native=helper.Native()
    native.api.RedrawWindow.argtypes=[w.HWND,c.c_void_p,w.HANDLE,w.UINT]; native.api.RedrawWindow.restype=w.BOOL
    native.api.ShowWindow.argtypes=[w.HWND,c.c_int]; native.api.ShowWindow.restype=w.BOOL
    native.api.GetAncestor.argtypes=[w.HWND,w.UINT]; native.api.GetAncestor.restype=w.HWND
    native.api.SetForegroundWindow.argtypes=[w.HWND]; native.api.SetForegroundWindow.restype=w.BOOL
    kernel=c.WinDLL('kernel32',use_last_error=True)
    kernel.GetProcessTimes.argtypes=[w.HANDLE]+[c.POINTER(w.FILETIME)]*4
    kernel.GetProcessTimes.restype=w.BOOL
    profile=repository/'.tmp'/('ui-responsiveness-'+str(uuid.uuid4()))
    (profile/'Panzo').mkdir(parents=True)
    (profile/'Panzo/settings.json').write_text(json.dumps({'theme':'dark','close_to_tray':False}),encoding='utf-8')
    screenshots=profile/'Videos/Panzo/Screenshots'; screenshots.mkdir(parents=True)
    Image.new('RGB',helper.IMAGE_SIZE,helper.FIXTURE_RGB).save(screenshots/'synthetic-audit.png')
    project=None
    if args.fixture_project:
        project=profile/'fixture.panzo'
        shutil.copytree(args.fixture_project.resolve(),project,ignore=shutil.ignore_patterns('*.lock','*.part.mp4', *([] if args.include_cache else ['cache'])))
    env=os.environ.copy(); env.update(LOCALAPPDATA=str(profile),USERPROFILE=str(profile),APPDATA=str(profile/'Roaming'))
    report={'binary':str(binary),'binarySha256':hashlib.sha256(binary.read_bytes()).hexdigest(),
            'profile':str(profile),'limit':'Single machine; Win32 message/API round trips, not display frames, FPS or input-to-photon latency. Idle CPU expressed as percent of one logical core. No pass threshold imposed.',
            'flushPendingPaint':args.flush_paint,'runs':[]}
    def stats(values):
        ordered=sorted(values)
        return {'count':len(values),'medianMs':round(statistics.median(values),3),
                'p95Ms':round(ordered[math.ceil(len(values)*.95)-1],3),
                'maxMs':round(max(values),3),'over50Ms':sum(x>50 for x in values),
                'rawMs':[round(x,3) for x in values]}
    def cpu_seconds(process):
        stamps=[w.FILETIME() for _ in range(4)]
        if not kernel.GetProcessTimes(w.HANDLE(int(process._handle)),*[c.byref(x) for x in stamps]):
            raise c.WinError(c.get_last_error())
        return sum((x.dwHighDateTime<<32)|x.dwLowDateTime for x in stamps[2:])/1e7
    def idle(process):
        before=cpu_seconds(process); start=time.perf_counter(); time.sleep(5)
        elapsed=time.perf_counter()-start
        used=cpu_seconds(process)-before
        return {'wallSeconds':round(elapsed,3),'cpuSeconds':round(used,4),
                'oneCorePercent':round(used/elapsed*100,3)}
    resize_phases=[]
    def position(root,width=1180,height=860):
        begin=time.perf_counter()
        if not native.api.SetWindowPos(root,w.HWND(-1),40,40,width,height,0x40):
            raise c.WinError(c.get_last_error())
        moved=time.perf_counter()
        native.send(root,0)
        barrier=time.perf_counter()
        if args.flush_paint: native.api.RedrawWindow(root,None,None,0x180) # UPDATENOW | ALLCHILDREN, no extra invalidation
        return [(moved-begin)*1000,(barrier-moved)*1000,(time.perf_counter()-barrier)*1000]
    def capture(root,name):
        position(root); time.sleep(.35)
        bounds=native.outer(root)
        # Own target's centre must be uncovered. This cannot prove every edge is unobscured.
        actual=native.api.WindowFromPoint(w.POINT((bounds[0]+bounds[2])//2,(bounds[1]+bounds[3])//2))
        owner=w.DWORD(); native.api.GetWindowThreadProcessId(actual,c.byref(owner))
        if owner.value!=process.pid or native.api.GetAncestor(actual,2)!=root:
            raise RuntimeError('Screenshot target is occluded (including owned windows); image not accepted')
        ImageGrab.grab(bbox=bounds,all_screens=True).save(out/name)
    def resize(root):
        times=[]
        for i in range(80):
            width=1000+(i%20)*16; height=740+(i%20)*8
            start=time.perf_counter(); phases=position(root,width,height); times.append((time.perf_counter()-start)*1000); resize_phases.append(phases)
            time.sleep(.016)
        result=stats(times)
        result['phases']={name:stats([row[index] for row in resize_phases]) for index,name in enumerate(['setWindowPos','messageBarrier','pendingPaint'])}
        resize_phases.clear()
        return result
    try:
        phases=([args.phase] if args.phase!='both' else ['image']+(['video'] if project else []))
        if 'video' in phases and project is None: raise ValueError('Video phase requires a fixture project')
        for phase in phases:
            process=None; root=None; run={'phase':phase}; report['runs'].append(run)
            try:
                startup=subprocess.STARTUPINFO(); startup.dwFlags|=subprocess.STARTF_USESHOWWINDOW; startup.wShowWindow=0
                process=subprocess.Popen([str(binary)]+([str(project)] if phase=='video' else []),cwd=repository,env=env,startupinfo=startup)
                run['pid']=process.pid
                root=native.find(process,'PanzoWorkspace',40); position(root)
                if phase=='image':
                    native.send(root,helper.WM_COMMAND,102)
                    history=native.find(process,'PanzoCaptureHistory',40,root)
                    native.send(history,helper.WM_COMMAND,12)
                    helper.wait_for_fixture(native,process,history,40)
                    native.send(history,helper.WM_COMMAND,3)
                    picture=native.find(process,'PanzoPicture',40,root)
                    position(root)
                    helper.wait_for_canvas(native,process,picture,40)
                    run['noInputVisible']=idle(process)
                    run['resize']=resize(root)
                    position(root)
                    for cmd in [100,101,102,100]: native.send(root,helper.WM_COMMAND,cmd); time.sleep(.1)
                    values=[]
                    for i in range(60):
                        start=time.perf_counter(); native.send(root,helper.WM_COMMAND,100+i%3); native.send(root,0)
                        values.append((time.perf_counter()-start)*1000); time.sleep(.03)
                    run['warmWorkspaceSwitch']=stats(values)
                    native.send(root,helper.WM_COMMAND,100)
                    native.api.ShowWindow(root,0); time.sleep(.5)
                    run['noInputHidden']=idle(process)
                    position(root)
                    native.api.SetWindowPos(root,w.HWND(-2),0,0,0,0,0x13)
                    native.send(root,helper.WM_COMMAND,115)
                    manager=native.find(process,'PanzoManagementWindow',10)
                    time.sleep(.6)
                    native.api.SetForegroundWindow(manager)
                    try: capture(manager,'plugins.png')
                    except RuntimeError as error: run['pluginScreenshotRejected']=str(error)
                    native.send(manager,helper.WM_CLOSE)
                else:
                    native.send(root,helper.WM_COMMAND,101); time.sleep(5)
                    run['noInputWarmupSamples']=[]
                    stable=0
                    for _ in range(12):
                        sample=idle(process); run['noInputWarmupSamples'].append(sample)
                        stable=stable+1 if sample['oneCorePercent']<2 else 0
                        if stable>=2: break
                    run['cpuSettled']=stable>=2
                    capture(root,'video-before.png')
                    run['noInputVisible']=idle(process)
                    run['resize']=resize(root)
                    capture(root,'video-after.png')
                    native.api.ShowWindow(root,0); time.sleep(.5)
                    run['noInputHidden']=idle(process)
                run['completed']=True
            finally:
                if process:
                    if process.poll() is None and root: native.api.PostMessageW(root,helper.WM_CLOSE,0,0)
                    try: process.wait(timeout=4)
                    except subprocess.TimeoutExpired:
                        process.terminate(); process.wait(timeout=5); run['cleanup']='Terminated only this isolated probe process after close grace period'
                    run['exitCode']=process.returncode
        report['completed']=True
    except Exception as error:
        report['completed']=False; report['error']=str(error)
    finally:
        (out/'responsiveness.json').write_text(json.dumps(report,ensure_ascii=False,indent=2),encoding='utf-8')
        print(json.dumps(report,ensure_ascii=False,indent=2))
    return 0 if report['completed'] else 1

if __name__=='__main__': raise SystemExit(main())

#!/usr/bin/env python3
"""Exercise UI uninstall while an unrelated plugin worker runs in a new isolated Panzo.
Requires the test-only plugin_lifecycle_fixture EXE. Never uses the user's profile,
registry, projects or existing processes. Synthetic output and PID evidence retained.
"""
import argparse, ctypes as c, hashlib, importlib.util, json, os, shutil, subprocess, time, uuid
from pathlib import Path
from ctypes import wintypes as w
from PIL import Image


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary',type=Path,required=True)
    parser.add_argument('--fixture',type=Path,required=True)
    parser.add_argument('--output',type=Path,required=True)
    args=parser.parse_args(); repo=Path(__file__).resolve().parent.parent
    binary=args.binary.resolve(); fixture=args.fixture.resolve(); out=args.output.resolve();out.mkdir(parents=True,exist_ok=True)
    spec=importlib.util.spec_from_file_location('panzo_native',repo/'scripts/test-drawing-stability.py')
    helper=importlib.util.module_from_spec(spec);spec.loader.exec_module(helper); n=helper.Native()
    n.api.IsWindowEnabled.argtypes=[w.HWND];n.api.IsWindowEnabled.restype=w.BOOL
    kernel=c.WinDLL('kernel32',use_last_error=True)
    kernel.OpenProcess.argtypes=[w.DWORD,w.BOOL,w.DWORD];kernel.OpenProcess.restype=w.HANDLE
    kernel.WaitForSingleObject.argtypes=[w.HANDLE,w.DWORD];kernel.WaitForSingleObject.restype=w.DWORD
    kernel.CloseHandle.argtypes=[w.HANDLE];kernel.CloseHandle.restype=w.BOOL
    profile=repo/'.tmp'/('plugin-hotplug-'+str(uuid.uuid4()))
    store=profile/'Panzo/Plugins';store.mkdir(parents=True)
    images=profile/'Videos/Panzo/Screenshots';images.mkdir(parents=True)
    original=images/'synthetic-original.png';Image.new('RGB',(256,160),(230,235,240)).save(original)
    digest=hashlib.sha256(original.read_bytes()).hexdigest()
    (profile/'Panzo/settings.json').write_text(json.dumps({'theme':'dark','close_to_tray':False}),encoding='utf-8')
    registry={'disabled':[],'installed':{},'previous':{}}
    dirs={}
    for plugin,label,command in [('review.alpha','Alpha Target','echo'),('review.beta','Beta Keep Running','hold')]:
        directory=str(uuid.uuid4());folder=store/directory;folder.mkdir();shutil.copy2(fixture,folder/'plugin.exe');dirs[plugin]=folder
        registry['installed'][plugin]={'directory':directory,'manifest':{'id':plugin,'name':label,'publisher':'Panzo test fixture','description':'Isolated test only','version':'1.0.0','protocol':1,'target':'x86_64-windows','commands':[{'id':command,'name':command}],'executable_sha256':hashlib.sha256(fixture.read_bytes()).hexdigest()}}
    (store/'registry.json').write_text(json.dumps(registry),encoding='utf-8')
    env=os.environ.copy();env.update(LOCALAPPDATA=str(profile),USERPROFILE=str(profile),APPDATA=str(profile/'Roaming'))
    process=None; root=None; worker_handle=None; marker=None
    report={'passed':False,'binary':str(binary),'binarySha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'profile':str(profile),'checks':[]}
    def wait(fn,timeout=12):
        end=time.monotonic()+timeout
        while time.monotonic()<end:
            if process.poll() is not None:raise RuntimeError(f'Isolated host exited: {process.returncode}')
            result=fn()
            if result:return result
            time.sleep(.025)
        raise RuntimeError('Probe condition timed out')
    def read_registry():
        # Persistence uses atomic replacement. A concurrent read may briefly lose
        # Windows sharing permission; retry the probe without changing the store.
        deadline=time.monotonic()+2
        while True:
            try:return json.loads((store/'registry.json').read_text(encoding='utf-8'))
            except (PermissionError,FileNotFoundError,json.JSONDecodeError):
                if time.monotonic()>=deadline:raise
                time.sleep(.025)
    def text(hwnd):
        value=c.create_unicode_buffer(2048);n.send(hwnd,13,len(value),c.addressof(value));return value.value
    def find(kind,parent=None,visible=True):
        return wait(lambda:next((h for h,k in n.windows(process.pid,parent) if k==kind and (not visible or n.api.IsWindowVisible(h))),None))
    def items(hwnd,combo=False):
        count=n.send(hwnd,0x146 if combo else 0x18B)
        result=[]
        for index in range(count):
            value=c.create_unicode_buffer(2048);n.send(hwnd,0x148 if combo else 0x189,index,c.addressof(value));result.append(value.value)
        return result
    def uninstall(manager,label):
        listing=n.api.GetDlgItem(manager,100);choices=items(listing)
        index=next(i for i,item in enumerate(choices) if label in item)
        n.send(listing,0x186,index)
        n.send(manager,0x111,(1<<16)|100,listing)
        # Confirmation belongs to this disposable process; never auto-accept user dialogs.
        n.api.PostMessageW(manager,0x111,5,0)
        dialog=find('PanzoMessageDialog')
        assert text(dialog)=='卸载插件',text(dialog)
        n.api.PostMessageW(dialog,0x111,1,0)
        wait(lambda:not n.api.IsWindowVisible(dialog))
    try:
        process=subprocess.Popen([str(binary)],cwd=repo,env=env)
        report['hostPidBefore']=process.pid
        root=find('PanzoWorkspace');report['hostHwnd']=root
        n.api.SetWindowPos(root,w.HWND(0),40,30,1180,860,0x40)
        n.send(root,0x111,102)
        history=find('PanzoCaptureHistory',root);n.send(history,0x111,41)
        helper.wait_for_fixture(n,process,history,15);n.send(history,0x111,3)
        find('PanzoPicture',root)
        panel=find('PanzoImageWorkspace',root)
        n.send(panel,0x111,11)
        form=find('PanzoWorkspaceForm',root)
        selector=n.api.GetDlgItem(form,100)
        choices=items(selector,True);index=next(i for i,item in enumerate(choices) if 'review.beta' in item)
        n.send(selector,0x14E,index);n.send(form,0x111,1)
        marker=wait(lambda:next(dirs['review.beta'].glob('started-*.json'),None))
        info=wait(lambda:json.loads(marker.read_text(encoding='utf-8')) if marker.stat().st_size else None)
        worker_pid=info['pid'];report['workerPidBefore']=worker_pid
        worker_handle=kernel.OpenProcess(0x100000,False,worker_pid)
        assert worker_handle and kernel.WaitForSingleObject(worker_handle,0)==258
        n.send(root,0x111,115)
        management=find('PanzoManagementWindow');manager=find('PanzoPluginManager',management)
        listing=n.api.GetDlgItem(manager,100);choices=items(listing)
        beta=next(i for i,item in enumerate(choices) if 'Beta' in item)
        n.send(listing,0x186,beta);n.send(manager,0x111,(1<<16)|100,listing)
        assert not n.api.IsWindowEnabled(n.api.GetDlgItem(manager,5)), 'running plugin uninstall must be disabled'
        assert not n.api.IsWindowEnabled(n.api.GetDlgItem(manager,4)), 'no rollback while in use'
        n.send(listing,0x186,0);n.send(manager,0x111,(1<<16)|100,listing)
        assert not n.api.IsWindowEnabled(n.api.GetDlgItem(manager,2)), 'already enabled builtin'
        assert not n.api.IsWindowEnabled(n.api.GetDlgItem(manager,4)), 'builtin rollback'
        assert not n.api.IsWindowEnabled(n.api.GetDlgItem(manager,5)), 'builtin uninstall'
        report['checks'].append('selection changes update enabled/disabled commands for builtins and running independent plugins')
        uninstall(manager,'Alpha Target')
        wait(lambda:'review.alpha' not in read_registry()['installed'])
        assert not dirs['review.alpha'].exists()
        assert process.poll() is None
        assert kernel.WaitForSingleObject(worker_handle,0)==258
        assert json.loads(marker.read_text())['pid']==worker_pid
        assert read_registry()['installed']['review.beta']==registry['installed']['review.beta']
        n.send(root,0) # Host window still responds through the same HWND.
        report['hostPidAfterUninstall']=process.pid;report['workerPidAfterUninstall']=worker_pid
        report['checks'].append('UI uninstall removes only alpha; host HWND/PID and beta process handle remain alive')
        assert hashlib.sha256(original.read_bytes()).hexdigest()==digest
        n.api.PostMessageW(management,0x10,0,0)
        suffix=marker.stem.removeprefix('started-')
        (marker.parent/f'release-{suffix}').write_text('go')
        wait(lambda:len(list(images.glob('*.png')))>=2)
        wait(lambda:kernel.WaitForSingleObject(worker_handle,0)==0)
        report['outputs']=[p.name for p in images.glob('*.png')]
        report['checks'].append('Unrelated beta task completes successfully and adds a new image after alpha uninstall')
        assert hashlib.sha256(original.read_bytes()).hexdigest()==digest
        n.send(root,0x111,115);find('PanzoManagementWindow');uninstall(manager,'Beta Keep Running')
        wait(lambda:'review.beta' not in read_registry()['installed'])
        assert len(list(images.glob('*.png')))>=2 and hashlib.sha256(original.read_bytes()).hexdigest()==digest
        report['checks'].append('Finished beta uninstalls without host restart; original and generated images retained')
        report['passed']=True
    except Exception as error:
        report['error']=str(error)
    finally:
        if marker and marker.exists():
            (marker.parent/f'release-{marker.stem.removeprefix("started-")}').write_text('go')
        if process:
            if root and process.poll() is None:n.api.PostMessageW(root,0x10,0,0)
            try:process.wait(timeout=4)
            except subprocess.TimeoutExpired:
                process.terminate();process.wait(timeout=5);report['cleanup']='Terminated only disposable host after close grace'
            report['exitCode']=process.returncode
        if worker_handle:kernel.CloseHandle(worker_handle)
        (out/'release-hotplug.json').write_text(json.dumps(report,ensure_ascii=False,indent=2),encoding='utf-8')
        print(json.dumps(report,ensure_ascii=False,indent=2))
    return 0 if report['passed'] else 1

if __name__=='__main__':raise SystemExit(main())

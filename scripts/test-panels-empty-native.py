"""Validate the empty video workspace and coalesced image inspector resizing in an isolated profile."""
import argparse,os,subprocess
from pathlib import Path
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('--output',type=Path,required=True)
args=parser.parse_args()
out=args.output.resolve();out.mkdir(parents=True,exist_ok=False)
profile=out/'profile';profile.mkdir(exist_ok=True)
for f in ['Videos','Pictures','Documents','Desktop','Downloads']:(profile/f).mkdir(exist_ok=True)
env=os.environ.copy();env.setdefault('CARGO_HOME',str(Path.home()/'.cargo'));env.setdefault('RUSTUP_HOME',str(Path.home()/'.rustup'));env.update(LOCALAPPDATA=str(profile),USERPROFILE=str(profile),APPDATA=str(profile/'Roaming'),PANZO_UI_REVIEW_DIR=str(out))
r=subprocess.run(['cargo','test','--offline','-p','panzo-windows','--lib','empty_video_workspace_actions','--','--ignored','--nocapture','--test-threads=1'],env=env,capture_output=True,text=True,encoding='utf-8',timeout=90)
(out/'run.txt').write_text(r.stdout+r.stderr,encoding='utf-8');print(r.stdout+r.stderr);raise SystemExit(r.returncode)

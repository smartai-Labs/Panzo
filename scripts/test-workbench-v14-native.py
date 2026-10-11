"""Native v14 layout checks. Only uses a disposable copy of the supplied project/profile."""
import argparse, os, shutil, subprocess, time
from pathlib import Path
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('--project',type=Path,required=True)
parser.add_argument('--package',type=Path,required=True)
parser.add_argument('--output',type=Path,required=True)
parser.add_argument('--project-assets-only',action='store_true')
parser.add_argument('--video-only',action='store_true')
parser.add_argument('--hover-only',action='store_true')
parser.add_argument('--panel-only',action='store_true')
parser.add_argument('--media-fixes-only',action='store_true')
parser.add_argument('--interaction-only',action='store_true')
parser.add_argument('--media-polish-only',action='store_true')
parser.add_argument('--preview-quality-only',action='store_true')
parser.add_argument('--inspector-only',action='store_true')
args=parser.parse_args()
repo=Path(__file__).resolve().parent.parent
out=args.output.resolve();out.mkdir(parents=True,exist_ok=False)
fixture=out/'fixture.panzo';shutil.copytree(args.project.resolve(),fixture)
profile=out/'profile';profile.mkdir()
for folder in ['Videos','Pictures','Documents','Desktop','Downloads']: (profile/folder).mkdir()
env=os.environ.copy();env.setdefault('CARGO_HOME',str(Path.home()/'.cargo'));env.setdefault('RUSTUP_HOME',str(Path.home()/'.rustup'));env.update(LOCALAPPDATA=str(profile),USERPROFILE=str(profile),APPDATA=str(profile/'Roaming'),PANZO_UI_REVIEW_PROJECT=str(fixture),PANZO_UI_REVIEW_DIR=str(out),PANZO_MEDIA_REVIEW_PACKAGE=str(args.package.resolve()))
if args.preview_quality_only: env['PANZO_PREVIEW_QUALITY_REVIEW']='1'
if args.media_polish_only: env['PANZO_MEDIA_POLISH_REVIEW']='1'
if args.inspector_only: env['PANZO_INSPECTOR_REVIEW']='1'
if args.interaction_only: env['PANZO_INTERACTION_REVIEW']='1'
if args.project_assets_only: env['PANZO_PROJECT_ASSET_REVIEW']='1'
if args.video_only: env['PANZO_VIDEO_ONLY']='1'
if args.hover_only: env['PANZO_HOVER_REVIEW']='1'
if args.panel_only: env['PANZO_PANEL_REVIEW']='1'
if args.media_fixes_only: env['PANZO_MEDIA_FIXES_REVIEW']='1'
run=subprocess.run(['cargo','test','--offline','-p','panzo-windows','--lib','workbench_v14_native_review','--','--ignored','--exact','platform::preview_window::workbench_v14_review::workbench_v14_native_review','--nocapture','--test-threads=1'],cwd=repo,env=env,capture_output=True,text=True,encoding='utf-8',timeout=180)
(out/'run.txt').write_text(run.stdout+run.stderr,encoding='utf-8')
(repo/'.tmp/v14-latest-review.txt').write_text(str(out),encoding='utf-8')
print(run.stdout+run.stderr)
raise SystemExit(run.returncode)

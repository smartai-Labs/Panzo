#!/usr/bin/env python3
"""Measure visible blanking during native image drawing on a Windows desktop.

Requires Pillow. Example (from the repository root):
  python scripts/test-drawing-stability.py --binary target/desktop-v3/release/panzo.exe --output docs/acceptance/drawing-release

The test opens an isolated Panzo process, creates a synthetic 4000 x 2500 image,
waits for a fully visible, stable canvas, then samples an untouched screen pixel
while drawing. Keep the desktop available during this test: other windows, the
lock screen, or moving/resizing the test window can invalidate the measurement.
It never attaches to, closes, or terminates an existing Panzo process. The unique
profile is retained for review. No forced redraw is used to make the test pass.

Pixel samples are NOT display frames or an FPS measurement. Zero changed samples
cannot exclude shorter flashes between samples. Exit codes: 0 passed, 1 failed.
"""
from __future__ import annotations

import argparse
import ctypes as c
from ctypes import wintypes as w
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import threading
import time
import uuid

IMAGE_SIZE = (4000, 2500)
FIXTURE_RGB = (232, 234, 238)
EXPECTED = FIXTURE_RGB[0] | (FIXTURE_RGB[1] << 8) | (FIXTURE_RGB[2] << 16)
WM_COMMAND, WM_CLOSE = 0x111, 0x10
WM_LBUTTONDOWN, WM_MOUSEMOVE, WM_LBUTTONUP = 0x201, 0x200, 0x202
LIMIT = (
    "Pixel sampling only; counts are not display frame counts or FPS. "
    "Zero changed samples cannot exclude shorter flashes between samples. "
    "Desktop occlusion or movement invalidates the observation."
)


def arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--binary", type=Path, required=True, help="Panzo executable to launch")
    parser.add_argument("--output", type=Path, required=True, help="Directory for JSON evidence and before/after screenshots")
    parser.add_argument("--duration", type=float, default=14.0, help="Minimum sampling duration in seconds (default: 14)")
    parser.add_argument("--moves", type=int, default=750, help="Number of mouse-move messages (default: 750)")
    parser.add_argument("--ready-timeout", type=float, default=40.0, help="Maximum seconds to wait for each readiness stage (default: 40)")
    args = parser.parse_args()
    if not 1 <= args.duration <= 60:
        parser.error("--duration must be between 1 and 60 seconds")
    if not 1 <= args.moves <= 5000:
        parser.error("--moves must be between 1 and 5000")
    if not 5 <= args.ready_timeout <= 120:
        parser.error("--ready-timeout must be between 5 and 120 seconds")
    args.binary = args.binary.resolve()
    args.output = args.output.resolve()
    if not args.binary.is_file():
        parser.error(f"Executable does not exist: {args.binary}")
    return args


class Native:
    def __init__(self) -> None:
        self.api = c.WinDLL("user32", use_last_error=True)
        self.gdi = c.WinDLL("gdi32", use_last_error=True)
        self.callback = c.WINFUNCTYPE(w.BOOL, w.HWND, w.LPARAM)
        signatures = {
            "GetWindowThreadProcessId": ([w.HWND, c.POINTER(w.DWORD)], w.DWORD),
            "GetClassNameW": ([w.HWND, w.LPWSTR, c.c_int], c.c_int),
            "GetDlgItem": ([w.HWND, c.c_int], w.HWND),
            "GetWindowRect": ([w.HWND, c.POINTER(w.RECT)], w.BOOL),
            "GetClientRect": ([w.HWND, c.POINTER(w.RECT)], w.BOOL),
            "ClientToScreen": ([w.HWND, c.POINTER(w.POINT)], w.BOOL),
            "SetWindowPos": ([w.HWND, w.HWND, c.c_int, c.c_int, c.c_int, c.c_int, w.UINT], w.BOOL),
            "GetDC": ([w.HWND], w.HDC),
            "ReleaseDC": ([w.HWND, w.HDC], c.c_int),
            "SendMessageTimeoutW": ([w.HWND, w.UINT, c.c_size_t, c.c_ssize_t, w.UINT, w.UINT, c.POINTER(c.c_size_t)], c.c_size_t),
            "PostMessageW": ([w.HWND, w.UINT, c.c_size_t, c.c_ssize_t], w.BOOL),
            "EnumWindows": ([self.callback, w.LPARAM], w.BOOL),
            "EnumChildWindows": ([w.HWND, self.callback, w.LPARAM], w.BOOL),
            "IsWindowVisible": ([w.HWND], w.BOOL),
            "WindowFromPoint": ([w.POINT], w.HWND),
        }
        for name, (types, result) in signatures.items():
            function = getattr(self.api, name)
            function.argtypes, function.restype = types, result
        self.gdi.GetPixel.argtypes = [w.HDC, c.c_int, c.c_int]
        self.gdi.GetPixel.restype = w.DWORD
        # Match ImageGrab's physical pixel coordinates on mixed-DPI desktops.
        try:
            awareness = self.api.SetProcessDpiAwarenessContext
            awareness.argtypes, awareness.restype = [c.c_void_p], w.BOOL
            awareness(c.c_void_p(-4))  # DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2
        except AttributeError:
            self.api.SetProcessDPIAware()

    def windows(self, pid: int, parent: int | None = None) -> list[tuple[int, str]]:
        values = []

        @self.callback
        def each(hwnd, _):
            owner = w.DWORD()
            self.api.GetWindowThreadProcessId(hwnd, c.byref(owner))
            if owner.value == pid:
                name = c.create_unicode_buffer(128)
                self.api.GetClassNameW(hwnd, name, len(name))
                values.append((hwnd, name.value))
            return True

        if parent:
            self.api.EnumChildWindows(parent, each, 0)
        else:
            self.api.EnumWindows(each, 0)
        return values

    def send(self, hwnd: int, message: int, wp: int = 0, lp: int = 0) -> int:
        result = c.c_size_t()
        if not self.api.SendMessageTimeoutW(hwnd, message, wp, lp, 2, 5000, c.byref(result)):
            raise RuntimeError(f"Message 0x{message:x} timed out for test HWND {hwnd}")
        return result.value

    def geometry(self, hwnd: int) -> tuple[int, int, int, int]:
        rect, origin = w.RECT(), w.POINT()
        if not self.api.GetClientRect(hwnd, c.byref(rect)) or not self.api.ClientToScreen(hwnd, c.byref(origin)):
            raise RuntimeError("Test canvas geometry is unavailable")
        return origin.x, origin.y, rect.right, rect.bottom

    def outer(self, hwnd: int) -> tuple[int, int, int, int]:
        rect = w.RECT()
        if not self.api.GetWindowRect(hwnd, c.byref(rect)):
            raise RuntimeError("Test window bounds are unavailable")
        return rect.left, rect.top, rect.right, rect.bottom

    def find(self, process: subprocess.Popen, kind: str, timeout: float, parent: int | None = None) -> int:
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            check_alive(process)
            found = [hwnd for hwnd, name in self.windows(process.pid, parent) if name == kind]
            if found:
                return found[0]
            time.sleep(0.05)
        raise RuntimeError(f"Test window not found: {kind}")


def check_alive(process: subprocess.Popen) -> None:
    if process.poll() is not None:
        raise RuntimeError(f"Isolated Panzo process exited with code {process.returncode}")


def packed_point(x: int, y: int) -> int:
    return ((int(y) & 0xffff) << 16) | (int(x) & 0xffff)


def wait_for_fixture(native: Native, process: subprocess.Popen, history: int, timeout: float) -> None:
    """The asset index is asynchronous; do not guess its completion with a sleep."""
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        check_alive(process)
        listing = native.api.GetDlgItem(history, 2)
        count = native.send(listing, 0x18B) if listing else 0  # LB_GETCOUNT
        if 0 < count < 0x7fffffff:
            native.send(listing, 0x186, 0)  # LB_SETCURSEL, first isolated fixture
            return
        time.sleep(0.08)
    raise RuntimeError("Synthetic image was not indexed before the readiness deadline")


def wait_for_canvas(native: Native, process: subprocess.Popen, picture: int, timeout: float):
    """Require the full uniform fixture rectangle, stable geometry and true screen pixels.

    Matching the fixture's aspect ratio rejects a partially painted/occluded right
    strip. Requiring a completely solid rectangle rejects stale pixels or controls.
    The candidate sample and stroke region must both belong to this picture HWND.
    """
    from PIL import Image, ImageChops, ImageGrab

    deadline = time.monotonic() + timeout
    stable_since = None
    previous = None
    last_reason = "canvas is not visible"
    started = time.monotonic()
    while time.monotonic() < deadline:
        check_alive(process)
        if not native.api.IsWindowVisible(picture):
            time.sleep(0.05)
            continue
        ox, oy, width, height = native.geometry(picture)
        if width < 120 or height < 120:
            last_reason = "canvas is too small"
            time.sleep(0.05)
            continue
        capture = ImageGrab.grab(bbox=(ox, oy, ox + width, oy + height), all_screens=True).convert("RGB")
        difference = ImageChops.difference(capture, Image.new("RGB", capture.size, FIXTURE_RGB))
        channels = difference.split()
        maximum = ImageChops.lighter(ImageChops.lighter(channels[0], channels[1]), channels[2])
        mask = maximum.point(lambda value: 255 if value == 0 else 0)
        bounds = mask.getbbox()
        valid = False
        if bounds:
            left, top, right, bottom = bounds
            bw, bh = right - left, bottom - top
            area = bw * bh
            valid = (
                bw >= 100 and bh >= 60
                and abs(bw - bh * IMAGE_SIZE[0] / IMAGE_SIZE[1]) <= 3
                and mask.histogram()[255] == area
            )
            if valid:
                sample = (ox + (left + right) // 2, oy + top + bh // 3)
                stroke_start = (left + bw // 4, top + bh * 7 // 10)
                stroke_width = max(12, min(178, bw * 2 // 5))
                stroke_height = max(3, min(12, bh // 20))
                probes = [sample, (ox + stroke_start[0], oy + stroke_start[1]),
                          (ox + stroke_start[0] + stroke_width, oy + stroke_start[1] + stroke_height)]
                dc = native.api.GetDC(None)
                if not dc:
                    raise RuntimeError("Cannot acquire desktop DC for readiness checks")
                try:
                    valid = all(
                        native.api.WindowFromPoint(w.POINT(*point)) == picture
                        and native.gdi.GetPixel(dc, *point) == EXPECTED
                        for point in probes
                    )
                finally:
                    native.api.ReleaseDC(None, dc)
        identity = (ox, oy, width, height, bounds)
        if valid:
            if identity != previous:
                stable_since = time.monotonic()
            elif stable_since is not None and time.monotonic() - stable_since >= 0.35:
                return {
                    "origin": [ox, oy], "clientSize": [width, height], "imageBoundsInClient": list(bounds),
                    "sample": sample, "strokeStart": stroke_start, "strokeWidth": stroke_width,
                    "strokeHeight": stroke_height, "waitMilliseconds": round((time.monotonic() - started) * 1000, 3),
                }
            previous = identity
        else:
            last_reason = f"fixture is not fully painted/unoccluded (bounds={bounds}, client={width}x{height})"
            previous, stable_since = None, None
        time.sleep(0.06)
    raise RuntimeError(f"Canvas failed readiness checks: {last_reason}")


def run(args: argparse.Namespace) -> int:
    from PIL import Image, ImageGrab

    repository = Path(__file__).resolve().parent.parent
    args.output.mkdir(parents=True, exist_ok=True)
    profile = repository / ".tmp" / ("drawing-stability-" + str(uuid.uuid4()))
    (profile / "Panzo").mkdir(parents=True)
    fixture_dir = profile / "Videos" / "Panzo" / "Screenshots"
    fixture_dir.mkdir(parents=True)
    (profile / "Panzo" / "settings.json").write_text(
        json.dumps({"theme": "dark", "close_to_tray": False}), encoding="utf-8"
    )
    fixture = fixture_dir / "synthetic-drawing.png"
    Image.new("RGB", IMAGE_SIZE, FIXTURE_RGB).save(fixture)
    native = Native()
    report = {
        "test": "native synthetic image drawing stability",
        "binary": str(args.binary), "binarySha256": hashlib.sha256(args.binary.read_bytes()).hexdigest(),
        "imageSize": list(IMAGE_SIZE), "sampled": False, "passed": False,
        "uniqueProfile": str(profile), "fixture": str(fixture),
        "requestedMinimumSeconds": args.duration, "requestedMoves": args.moves,
        "samplingIntervalRequestedMilliseconds": 2, "limit": LIMIT,
    }
    startup = subprocess.STARTUPINFO()
    startup.dwFlags |= subprocess.STARTF_USESHOWWINDOW
    startup.wShowWindow = 0
    environment = os.environ.copy()
    environment.update(LOCALAPPDATA=str(profile), USERPROFILE=str(profile), APPDATA=str(profile / "Roaming"))
    process = None
    root = None
    sample_thread = None
    samples: list[tuple[float, int]] = []
    sample_errors: list[str] = []
    sampling_started, drawing_done, stop_sampling = threading.Event(), threading.Event(), threading.Event()
    try:
        process = subprocess.Popen([str(args.binary)], cwd=repository, env=environment, startupinfo=startup)
        report["isolatedProcessId"] = process.pid
        root = native.find(process, "PanzoWorkspace", args.ready_timeout)
        if not native.api.SetWindowPos(root, w.HWND(-1), 50, 50, 1100, 800, 0x40):  # TOPMOST | SHOWWINDOW
            raise RuntimeError("Could not position the isolated test window")
        native.send(root, WM_COMMAND, 102)  # Assets workspace
        history = native.find(process, "PanzoCaptureHistory", args.ready_timeout, root)
        native.send(history, WM_COMMAND, 12)  # Image category
        wait_for_fixture(native, process, history, args.ready_timeout)
        native.send(history, WM_COMMAND, 3)  # Explicitly edit selected asset
        picture = native.find(process, "PanzoPicture", args.ready_timeout, root)
        # StartupInfo can hide the app on its first ShowWindow call. Wait until
        # the image exists (the message loop is running), then position our own
        # test window; an earlier SetWindowPos may be undone by startup.
        if not native.api.SetWindowPos(root, w.HWND(-1), 50, 50, 1100, 800, 0x40):
            raise RuntimeError("Could not show the ready isolated test window")
        # The active tool changes the contextual toolbar height. Set it before
        # choosing the untouched sample point and recording window geometry.
        native.send(picture, WM_COMMAND, 3)  # Pen
        ready = wait_for_canvas(native, process, picture, args.ready_timeout)
        report["readiness"] = ready
        sample_point = ready["sample"]
        report["unchangedPixel"] = list(sample_point)
        report["expectedColorRef"] = EXPECTED
        original_outer = native.outer(root)
        ImageGrab.grab(bbox=original_outer, all_screens=True).save(args.output / "drawing-before.png")
        native.send(picture, WM_COMMAND, 3)  # Pen

        def sample() -> None:
            dc = native.api.GetDC(None)
            if not dc:
                sample_errors.append("Could not acquire the sampling thread's desktop DC")
                sampling_started.set()
                return
            start = time.perf_counter()
            sampling_started.set()
            try:
                while not stop_sampling.is_set():
                    elapsed = time.perf_counter() - start
                    if elapsed >= args.duration and drawing_done.is_set():
                        break
                    check_alive(process)
                    pixel = int(native.gdi.GetPixel(dc, *sample_point))
                    samples.append((round(elapsed * 1000, 3), pixel))
                    time.sleep(0.002)
            except Exception as error:
                sample_errors.append(str(error))
            finally:
                native.api.ReleaseDC(None, dc)

        sample_thread = threading.Thread(target=sample, name="Panzo-drawing-pixel-sampler", daemon=True)
        sample_thread.start()
        if not sampling_started.wait(timeout=3):
            raise RuntimeError("Sampling thread did not start")
        if sample_errors:
            raise RuntimeError(sample_errors[0])
        time.sleep(0.2)
        sx, sy = ready["strokeStart"]
        width, height = ready["strokeWidth"], ready["strokeHeight"]
        native.send(picture, WM_LBUTTONDOWN, 1, packed_point(sx, sy))
        last = (sx, sy)
        move_start = time.perf_counter()
        for index in range(args.moves):
            last = (sx + (index % 90) * width // 89, sy + (index % 12) * height // 11)
            native.send(picture, WM_MOUSEMOVE, 1, packed_point(*last))
            if sample_errors:
                raise RuntimeError(sample_errors[0])
            time.sleep(0.016)
        native.send(picture, WM_LBUTTONUP, 0, packed_point(*last))
        report["actualMoves"] = args.moves
        report["drawingMilliseconds"] = round((time.perf_counter() - move_start) * 1000, 3)
        time.sleep(0.2)
        drawing_done.set()
        sample_thread.join(timeout=args.duration + 5)
        if sample_thread.is_alive():
            raise RuntimeError("Sampling thread did not finish")
        if sample_errors:
            raise RuntimeError(sample_errors[0])
        if not samples:
            raise RuntimeError("No screen pixel samples were collected")
        if native.outer(root) != original_outer or list(native.geometry(picture)) != ready["origin"] + ready["clientSize"]:
            report["geometryChange"]={"beforeRoot":original_outer,"afterRoot":native.outer(root),"beforePicture":ready["origin"]+ready["clientSize"],"afterPicture":native.geometry(picture)}
            raise RuntimeError("Window geometry changed during sampling; measurement is invalid")
        if native.api.WindowFromPoint(w.POINT(*sample_point)) != picture:
            raise RuntimeError("Another window obscured the sample point; measurement is invalid")
        ImageGrab.grab(bbox=original_outer, all_screens=True).save(args.output / "drawing-after.png")
        changed = [row for row in samples if row[1] != EXPECTED]
        report.update(
            sampled=True, passed=not changed, samples=len(samples), changedSamples=len(changed),
            observedColors=sorted({row[1] for row in samples}), firstChanges=changed[:20],
            samplingMilliseconds=samples[-1][0],
        )
        if changed:
            report["error"] = "Untouched image pixel changed while drawing; inspect screenshots and raw samples"
    except (Exception, KeyboardInterrupt) as error:
        report["error"] = str(error) or type(error).__name__
        report["passed"] = False
        if root and process and process.poll() is None:
            try:
                ImageGrab.grab(bbox=native.outer(root), all_screens=True).save(args.output / "drawing-failure.png")
            except Exception as capture_error:
                report["failureScreenshotError"] = str(capture_error)
    finally:
        drawing_done.set()
        stop_sampling.set()
        if sample_thread is not None:
            sample_thread.join(timeout=5)
        if samples:
            (args.output / "drawing-samples.json").write_text(json.dumps(samples), encoding="utf-8")
            report.setdefault("samples", len(samples))
        if sample_errors:
            report["samplingErrors"] = sample_errors
        # Only the Popen child launched above is eligible for shutdown. WM_CLOSE
        # can request confirmation for the synthetic unsaved stroke, so force an
        # exit of this exact child after a grace period. Never enumerate/kill apps.
        if process is not None:
            if process.poll() is None:
                if root:
                    # Persist only this disposable fixture, so cleanup also exercises a normal exit.
                    try:
                        own_picture = next((h for h,k in native.windows(process.pid,root) if k == "PanzoPicture"),None)
                        if own_picture: native.send(own_picture, WM_COMMAND, 44)
                    except Exception as error:
                        report["cleanupSaveError"] = str(error)
                    native.api.PostMessageW(root, WM_CLOSE, 0, 0)
                try:
                    process.wait(timeout=4)
                    report["cleanup"] = "isolated child exited after WM_CLOSE"
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=10)
                    report["cleanup"] = "terminated only the isolated child after close grace period"
            else:
                report["cleanup"] = "isolated child already exited"
            report["processExitCode"] = process.returncode
        (args.output / "drawing-audit.json").write_text(
            json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8"
        )
    print(json.dumps(report, ensure_ascii=False))
    return 0 if report["passed"] else 1


def main() -> int:
    args = arguments()
    if os.name != "nt":
        print("This test requires a Windows desktop.", file=sys.stderr)
        return 1
    try:
        return run(args)
    except ImportError as error:
        print(f"Missing test dependency: {error}. Install Pillow for the test interpreter.", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())

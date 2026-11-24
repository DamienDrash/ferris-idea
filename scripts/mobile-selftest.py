#!/usr/bin/env python3
"""Pairs the debug iOS or Android app with a real `ferris-idea-serve serve --pair` (SPEC-0326).

The host runs under a pseudo-terminal, as `serve --pair` confirms digits only at a terminal. On
macOS `--key-file` is ignored and a release build uses the login keychain, so this needs a debug
build: only those keep the host's key in a file, here in a scratch folder.
Prints the app's SELFTEST lines; exits 0 only when the app passed and both sides showed the same
digits.

usage: mobile-selftest.py <ferris-idea-serve binary> <simulator UDID> <project folder> <file in it>
       [<screenshot.png>]
       mobile-selftest.py --android <adb serial> <ferris-idea-serve binary> <project folder>
       <file in it> [<screenshot.png>]

On Android the app reaches the host's loopback listener as 10.0.2.2, the emulator's address for
the Mac, and gets its inputs as intent extras; its SELFTEST lines come from logcat. The pairing
code is on adb's command line for the moment of the launch.
"""
import os
import pty
import shlex
import re
import select
import socket
import atexit
import subprocess
import sys
import shutil
import tempfile
import time

BUNDLE = "dev.ferrisidea.mobile"
DEADLINE = 120


def launch_ios(udid, address, code, folder, file_name, log):
    env = {
        **os.environ,
        "SIMCTL_CHILD_FERRIS_IDEA_SELFTEST_ADDRESS": address,
        "SIMCTL_CHILD_FERRIS_IDEA_SELFTEST_CODE": code,
        "SIMCTL_CHILD_FERRIS_IDEA_SELFTEST_FOLDER": folder,
        "SIMCTL_CHILD_FERRIS_IDEA_SELFTEST_FILE": file_name,
    }
    return subprocess.Popen(
        ["xcrun", "simctl", "launch", "--console-pty",
         "--terminate-running-process", udid, BUNDLE],
        stdout=log, stderr=subprocess.STDOUT, stdin=subprocess.DEVNULL, env=env,
    )


def launch_android(serial, address, code, folder, file_name, log):
    adb = ["adb", "-s", serial]
    subprocess.run(adb + ["shell", "am", "force-stop", BUNDLE], check=True)
    subprocess.run(adb + ["logcat", "-c"], check=True)
    logcat = subprocess.Popen(
        adb + ["logcat", "-v", "raw", "-s", "FerrisIDEASelfTest:I", "AndroidRuntime:E"],
        stdout=log, stderr=subprocess.STDOUT, stdin=subprocess.DEVNULL,
    )
    extras = []
    for key, value in [("address", address), ("code", code), ("folder", folder),
                       ("file", file_name)]:
        extras += ["--es", f"selftest_{key}", shlex.quote(value)]
    subprocess.run(adb + ["shell", "am", "start", "-n", f"{BUNDLE}/.MainActivity"] + extras,
                   check=True, stdout=subprocess.DEVNULL)
    return logcat


def screenshot(android, device, path):
    if android:
        with open(path, "wb") as f:
            subprocess.run(["adb", "-s", device, "exec-out", "screencap", "-p"], stdout=f,
                           check=False)
    else:
        subprocess.run(["xcrun", "simctl", "io", device, "screenshot", path], check=False)


def main():
    args = sys.argv[1:]
    android = args[:1] == ["--android"]
    if android:
        device, serve, folder, file_name = args[1:5]
        shot = args[5] if len(args) > 5 else None
    else:
        serve, device, folder, file_name = args[0:4]
        shot = args[4] if len(args) > 4 else None
    if "/debug/" not in os.path.realpath(serve):
        sys.exit("refused: only a debug build keeps its key out of the login keychain")
    state = tempfile.mkdtemp(prefix="ferris-idea-selftest-")
    os.chmod(state, 0o700)
    # The folder holds the test host's key: it goes when the script ends, also on failure.
    atexit.register(shutil.rmtree, state, True)
    # A pin belongs to an address, and each run's host has a new key: a new port for each run.
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        port = str(probe.getsockname()[1])
    master, slave = pty.openpty()
    host = subprocess.Popen(
        [serve, "serve", "--pair", "--listen", f"127.0.0.1:{port}", "--key-file",
         "--grant", "view", folder],
        stdin=slave, stdout=slave, stderr=slave, close_fds=True,
        env={**os.environ, "FERRIS_IDEA_CONFIG_DIR": state},
    )
    os.close(slave)
    output = ""
    app = None
    host_digits = None
    answered = False
    log = tempfile.NamedTemporaryFile("w+", prefix="app-", suffix=".log", dir=state, delete=False)
    start = time.time()
    code = None
    lines = []
    try:
        while time.time() - start < DEADLINE:
            if select.select([master], [], [], 0.2)[0]:
                try:
                    chunk = os.read(master, 4096).decode(errors="replace")
                except OSError:
                    break
                output += chunk
            if code is None:
                m = re.search(r"address \S+ and code (\w+)", output)
                if m:
                    code = m.group(1)
                    launch = launch_android if android else launch_ios
                    host_address = "10.0.2.2" if android else "127.0.0.1"
                    app = launch(device, f"{host_address}:{port}", code,
                                 os.path.realpath(folder), file_name, log)
            m = re.search(r"Digits: (\d{6})", output)
            if m and not answered:
                host_digits = m.group(1)
                os.write(master, b"y\n")
                answered = True
            log.flush()
            with open(log.name) as f:
                lines = [l.strip() for l in f if "SELFTEST" in l or "FATAL" in l]
            if any("PASS" in l or " FAIL" in l or "FATAL" in l for l in lines):
                break
        if shot and any(l.endswith("PASS") for l in lines):
            time.sleep(3)
            screenshot(android, device, shot)
    finally:
        with open(log.name) as f:
            lines = [l.strip() for l in f if "SELFTEST" in l]
        host.terminate()
        if app:
            app.terminate()
        # The host must be gone before its folder is removed, or it could write its key again.
        try:
            host.wait(timeout=10)
        except subprocess.TimeoutExpired:
            host.kill()
            host.wait()
    if not any(l.endswith("PASS") for l in lines):
        shown = output.replace(code or "\0", "<code>")
        print("host said:", shown[-800:])
        with open(log.name) as f:
            print("app said:", f.read()[-800:])
    # The pairing code and any line carrying it stay out of the output.
    for line in lines:
        print(line)
    digits = next((re.search(r"digits=(\d+)", l).group(1) for l in lines if "digits=" in l), None)
    same = digits is not None and digits == host_digits
    print(f"host digits match app digits: {same}")
    passed = any(l.endswith("PASS") for l in lines) and same
    print("RESULT", "PASS" if passed else "FAIL")
    sys.exit(0 if passed else 1)


main()

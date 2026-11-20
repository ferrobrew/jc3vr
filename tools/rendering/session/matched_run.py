"""Run a matched capture schedule in a fresh private session from the repository root."""

import argparse
import json
import os
from pathlib import Path
import select
import shutil
import signal
import subprocess
import sys
import time

from tools.rendering.experiment import ExperimentConfig


BASE = Path('target/harness-probe')
ROOT = Path(__file__).resolve().parents[3]
CLI = [sys.executable, '-m', 'tools.rendering']


def session_args(number, gpu, extra=()):
    """The session command line; `extra` carries the eye-size options of a session launched at
    another size than the harness's default one."""
    return CLI + ['session', '--no-build', '--gpu', gpu, '--output', str(BASE / f'persistent-{number}'),
                  *extra]


def recorded_session_args(number, gpu):
    """The arguments session `number` was launched with, from its launch request when it recorded
    one (a session launched at another eye size carries its scale), else the default ones."""
    request = BASE / f'diagnostics-{number}/launch-request.json'
    if request.is_file():
        return json.loads(request.read_text())['session']
    return session_args(number, gpu)


def stop_prior(number, gpu, output):
    pid = int((BASE / f'diagnostics-{number}/controller.pid').read_text())
    fd = os.pidfd_open(pid)
    try:
        proc = Path('/proc') / str(pid)
        argv = os.fsdecode((proc / 'cmdline').read_bytes()).removesuffix('\0').split('\0')
        cwd = (proc / 'cwd').resolve(strict=True)
        if cwd != ROOT or argv != recorded_session_args(number, gpu):
            raise RuntimeError(f'controller: PID {pid} does not match the previous session')
        with (output / 'prior-controller.json').open('x') as stream:
            json.dump(dict(pid=pid, args=argv, cwd=str(cwd)), stream)
        watch = select.poll()
        watch.register(fd, select.POLLIN)
        signal.pidfd_send_signal(fd, signal.SIGTERM)
        if not watch.poll(90000):
            raise RuntimeError(f'controller: PID {pid} did not exit')
    finally:
        os.close(fd)


def command(output, name, args):
    with (output / f'{name}.log').open('xb') as log:
        result = subprocess.run(CLI + args, stdout=log, stderr=subprocess.STDOUT, timeout=7200)
    (output / f'{name}.exit').write_text(f'{result.returncode}\n')
    return result.returncode


# The checks whose exit codes a launch requires by default, and the one an inner-loop launch
# (`--build-only`) requires: the full checks run once before committing, not before every launch.
FULL_CHECKS = ('fmt', 'build', 'clippy', 'test', 'python')
BUILD_CHECKS = ('build',)


def run(number, config, gpu, prior, unmatched=False, extra=(), checks=FULL_CHECKS):
    """Launch session `number` (stopping session `prior` first) and, given a `config`, run it as a
    paired experiment; without one, return once the session is in gameplay."""
    output = BASE / f'diagnostics-{number}'
    session = BASE / f'persistent-{number}'
    if Path.cwd() != ROOT or session.exists() or (output / 'paired').exists():
        raise RuntimeError('matched run: expected repository root and unused session and capture paths')
    if config is not None and not unmatched and not ExperimentConfig.load(config).matched_treatment:
        raise ValueError('matched run: configuration has no matched treatment')
    for check in checks:
        if (output / f'{check}.exit').read_text().strip() != '0':
            raise RuntimeError(f'matched run: {check} did not pass')
    if shutil.disk_usage(BASE).free <= 16 * 2**30:
        raise RuntimeError('matched run: less than 16 GiB of capture space available')
    with (output / 'launch-request.json').open('x') as stream:
        json.dump(dict(config=None if config is None else str(config), prior=prior,
                       checks=list(checks), session=session_args(number, gpu, extra)), stream)
    if prior is not None:
        stop_prior(prior, gpu, output)
    with (output / 'controller.log').open('xb') as log:
        child = subprocess.Popen(session_args(number, gpu, extra), stdin=subprocess.DEVNULL, stdout=log,
                                 stderr=subprocess.STDOUT, start_new_session=True)
    (output / 'controller.pid').write_text(f'{child.pid}\n')
    deadline = time.monotonic() + 150
    while time.monotonic() < deadline:
        if child.poll() is not None:
            raise RuntimeError(f'controller: exited before gameplay ({child.returncode}); see {output}')
        ready = next((line for f in session.glob('sessions/*/session.log')
                      for line in f.read_text(errors='replace').splitlines() if 'camera height above Rico' in line), None)
        if ready and (session / 'runner.json').is_file():
            (output / 'gameplay-ready.log').write_text(ready + '\n')
            break
        time.sleep(1)
    else:
        raise RuntimeError(f'controller: gameplay readiness timed out; see {output}')
    if config is None:
        return 0
    paired = output / 'paired'
    rc = command(output, 'paired', ['run', '--runner', str(session / 'runner.json'), '--config', str(config),
                                    '--output', str(paired), '--timeout', '420'])
    if (paired / 'manifest.json').is_file():
        stages = command(output, 'regions', ['stages', str(paired / 'manifest.json'), '--output',
                                            str(output / 'regions'), '--water-regions'])
        return rc or stages
    return rc or 2


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('number', type=int)
    parser.add_argument('config', type=Path, nargs='?',
                        help='the experiment to run once the session is in gameplay; without one, '
                             'only launch')
    parser.add_argument('--gpu', required=True)
    controller = parser.add_mutually_exclusive_group(required=True)
    controller.add_argument('--prior', type=int)
    controller.add_argument('--fresh', action='store_true')
    parser.add_argument('--unmatched', action='store_true',
                        help='accept a configuration without a matched treatment')
    parser.add_argument('--resolution-scale', help='passed to the session (see its --help)')
    parser.add_argument('--eye-size', help='passed to the session (see its --help)')
    parser.add_argument('--compositor-fps', help='passed to the session (see its --help)')
    parser.add_argument('--build-only', action='store_true',
                        help='require only build.exit, for the inner loop; the full checks still '
                             'run before committing')
    args = parser.parse_args()
    extra = [item for name, value in (('--resolution-scale', args.resolution_scale), ('--eye-size', args.eye_size),
                                      ('--compositor-fps', args.compositor_fps))
             if value is not None for item in (name, value)]
    sys.exit(run(args.number, args.config, args.gpu, args.prior, args.unmatched, extra,
                 BUILD_CHECKS if args.build_only else FULL_CHECKS))

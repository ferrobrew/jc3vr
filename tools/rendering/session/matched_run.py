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


def session_args(number, gpu):
    return CLI + ['session', '--no-build', '--gpu', gpu, '--output', str(BASE / f'persistent-{number}')]


def stop_prior(number, gpu, output):
    pid = int((BASE / f'diagnostics-{number}/controller.pid').read_text())
    fd = os.pidfd_open(pid)
    try:
        proc = Path('/proc') / str(pid)
        argv = os.fsdecode((proc / 'cmdline').read_bytes()).removesuffix('\0').split('\0')
        cwd = (proc / 'cwd').resolve(strict=True)
        if cwd != ROOT or argv != session_args(number, gpu):
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


def run(number, config, gpu, prior):
    output = BASE / f'diagnostics-{number}'
    session = BASE / f'persistent-{number}'
    if Path.cwd() != ROOT or session.exists() or (output / 'paired').exists():
        raise RuntimeError('matched run: expected repository root and unused session and capture paths')
    if not ExperimentConfig.load(config).matched_treatment:
        raise ValueError('matched run: configuration has no matched treatment')
    for check in ('fmt', 'build', 'clippy', 'test', 'python'):
        if (output / f'{check}.exit').read_text().strip() != '0':
            raise RuntimeError(f'matched run: {check} did not pass')
    if shutil.disk_usage(BASE).free <= 16 * 2**30:
        raise RuntimeError('matched run: less than 16 GiB of capture space available')
    with (output / 'launch-request.json').open('x') as stream:
        json.dump(dict(config=str(config), prior=prior, session=session_args(number, gpu)), stream)
    if prior is not None:
        stop_prior(prior, gpu, output)
    with (output / 'controller.log').open('xb') as log:
        child = subprocess.Popen(session_args(number, gpu), stdin=subprocess.DEVNULL, stdout=log,
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
    parser.add_argument('config', type=Path)
    parser.add_argument('--gpu', required=True)
    controller = parser.add_mutually_exclusive_group(required=True)
    controller.add_argument('--prior', type=int)
    controller.add_argument('--fresh', action='store_true')
    args = parser.parse_args()
    sys.exit(run(args.number, args.config, args.gpu, args.prior))

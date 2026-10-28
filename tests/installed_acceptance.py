#!/usr/bin/env python3
"""Installed, developer-tool-free runtime checks; run as the unprivileged app user."""
import os
import shutil
import subprocess

for tool in ('cargo', 'rustc', 'node', 'gcc'):
    assert shutil.which(tool) is None, f'Developer tool unexpectedly present: {tool}'
assert os.geteuid() != 0, 'Runtime acceptance must not run as root'
for command in [
    ['python3', 'tests/acceptance.py'],
    ['python3', 'tests/media_acceptance.py'],
    ['python3', 'tests/io_failures.py'],
    ['dbus-run-session', '--', 'xvfb-run', '-a', 'python3', 'tests/desktop_webdriver.py'],
]:
    print('RUN', ' '.join(command), flush=True)
    subprocess.run(command, check=True)

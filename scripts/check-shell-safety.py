#!/usr/bin/env python3
"""Offline regression checks for shell-facing environment and cleanup contracts."""
from pathlib import Path
import base64
import json
import os
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parent.parent
PARSER = ROOT / 'scripts/env-data.py'


def test_environment() -> None:
    with tempfile.TemporaryDirectory(prefix='jelly-shell-safety-') as directory:
        path = Path(directory) / '.env'
        marker = Path(directory) / 'unexpected-execution'
        values = {
            'JELLY_MCP_TOKEN': f'$(touch {marker}) spaces # quote " and $HOME',
            'JELLY_BOOTSTRAP_SECRET': "'backslash\\value'",
            'JELLY_PUBLIC_URL': 'https://example.com/?a=1&b=2',
            'JELLY_TELEGRAM_CHAT_ID': 'é-special',
        }
        subprocess.run(['python3', str(PARSER), 'write', str(path)],
                       input=json.dumps(values), text=True, check=True)
        assert path.stat().st_mode & 0o777 == 0o600
        result = subprocess.check_output(['python3', str(PARSER), 'read', str(path)], text=True)
        restored = {name: base64.b64decode(encoded).decode()
                    for name, encoded in (row.split('\t', 1) for row in result.splitlines())}
        assert restored == values
        assert not marker.exists(), 'the parser must never execute shell syntax'
        assert all(k.startswith('JELLY_') for k in restored)
        # Exercise the actual shell config-loader against an isolated fake root.
        # No Jelly service or browser is started.
        fake = Path(directory)
        (fake / 'scripts').mkdir()
        (fake / 'config').mkdir()
        shutil.copy2(PARSER, fake / 'scripts/env-data.py')
        for name in ('jelly.toml', 'cargo.toml'):
            shutil.copy2(ROOT / 'config' / name, fake / 'config' / name)
        env = os.environ.copy()
        env['REPO_ROOT'] = str(fake)
        env['JELLY_PUBLIC_URL'] = 'https://override.example'
        env.pop('JELLY_MCP_TOKEN', None)
        env.pop('JELLY_BOOTSTRAP_SECRET', None)
        output = subprocess.check_output([
            'bash', '-c', 'set -euo pipefail; source "$1"; printf "%s\\n%s\\n" "$JELLY_MCP_TOKEN" "$JELLY_PUBLIC_URL"',
            'bash', str(ROOT / 'scripts/config.sh')], env=env, text=True)
        assert output.splitlines() == [values['JELLY_MCP_TOKEN'], 'https://override.example']
        assert not marker.exists(), 'the Bash config-loader must never execute input'
        path.write_text('JELLY_MCP_TOKEN=$(touch ' + str(marker) + ')\n')
        subprocess.check_call(['python3', str(PARSER), 'read', str(path)], stdout=subprocess.DEVNULL)
        assert not marker.exists(), 'even legacy unquoted input must be inert'
        path.write_text('JELLY_MCP_TOKEN="unterminated\n')
        bad = subprocess.run(['python3', str(PARSER), 'read', str(path)], capture_output=True)
        assert bad.returncode != 0, 'malformed input must fail closed'
        path.write_text('PATH=/tmp/unsafe\n')
        bad = subprocess.run(['python3', str(PARSER), 'read', str(path)], capture_output=True)
        assert bad.returncode != 0, 'non-JELLY variables must be rejected'


def test_deletion_guard() -> None:
    source = (ROOT / 'scripts/clean-runtime.sh').read_text()
    assert 'validate_cleanup_target "$RUNTIME"' in source
    assert 'validate_cleanup_target "$BUILD_DIR"' in source
    guard = source.split('validate_cleanup_target() {', 1)[1].split('\n}', 1)[0]
    script = '''set -euo pipefail
ROOT=/data/github/jelly
HOME=/home/test
validate_cleanup_target() {''' + guard + '''
}
for target in / /data /etc /tmp /home /data/github/jelly /data/other-project /data/../data; do
    if validate_cleanup_target "$target" >/dev/null 2>&1; then
        echo "UNSAFE ALLOWED: $target" >&2
        exit 2
    fi
done
validate_cleanup_target /data/jelly-runtime
validate_cleanup_target /data/.jelly-build
'''
    subprocess.run(['bash', '-c', script], check=True)


def main() -> None:
    test_environment()
    test_deletion_guard()
    print('PASS: safe environment parsing/serialization, permissions, non-execution, and guarded cleanup paths')


if __name__ == '__main__':
    main()

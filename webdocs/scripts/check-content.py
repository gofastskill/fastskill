#!/usr/bin/env python3
"""Check authored examples. Optional binary validates flags without executing examples."""
import argparse
import json
from pathlib import Path
import re
import shlex
import subprocess
import tomllib

ROOT = Path(__file__).resolve().parents[1]


def pages():
    return sorted(p for p in ROOT.rglob('*.mdx')
                  if not any(part in {'node_modules', '.next', '.source', 'out'} for part in p.relative_to(ROOT).parts))


def fences(text, languages):
    pattern = r'^```(' + '|'.join(languages) + r')(?:[^\n]*)\n(.*?)^```'
    for match in re.finditer(pattern, text, re.M | re.S):
        yield text[:match.start()].count('\n') + 1, match.group(2)


def check_config(data):
    config = data.get('tool', {}).get('fastskill', {})
    repos = config.get('repositories', [])
    if not isinstance(repos, list):
        raise ValueError('tool.fastskill.repositories must be an array of tables')
    required = {'http-registry': 'index_url', 'git-marketplace': 'url', 'zip-url': 'zip_url', 'local': 'path'}
    for repo in repos:
        kind = repo.get('type')
        if kind not in required:
            raise ValueError(f'unknown repository type {kind}')
        for key in ('name', required[kind]):
            if not isinstance(repo.get(key), str) or not repo[key]:
                raise ValueError(f'{kind} repository requires string {key}')
        if not isinstance(repo.get('priority'), int) or repo['priority'] < 0:
            raise ValueError('repository priority must be a nonnegative integer')
        auth = repo.get('auth')
        if auth and (auth.get('type') != 'pat' or not isinstance(auth.get('env_var'), str)):
            raise ValueError('repository auth supports pat with an env_var string')


def check(binary=None):
    errors, configs, commands = [], 0, 0
    spec = None
    help_flags = {}
    if binary:
        spec = json.loads(subprocess.check_output([binary, 'cli', 'spec', '--format', 'json'], text=True))
        expected = tomllib.loads((ROOT.parent / 'Cargo.toml').read_text())['workspace']['package']['version']
        if spec['app']['version'] != expected:
            raise ValueError(f'CLI version {spec["app"]["version"]} does not match {expected}')
    for page in pages():
        text = page.read_text()
        for line, block in fences(text, ['toml']):
            configs += 1
            try:
                check_config(tomllib.loads(block))
            except (ValueError, TypeError, KeyError) as error:
                errors.append(f'{page.relative_to(ROOT)}:{line}: {error}')
        if not spec:
            continue
        for line, block in fences(text, ['bash', 'shell', 'powershell']):
            for command in block.replace('\\\n', ' ').splitlines():
                command = re.sub(r'^\s*\$\s+', '', command).strip()
                if not command.startswith('fastskill '):
                    continue
                try:
                    words = shlex.split(command, comments=True)
                except ValueError:
                    continue  # Multi-line quoted data is covered by the journey runner.
                matches = [c for c in spec['commands'] if words[1:1+len(c['path'].split('/'))] == c['path'].split('/')]
                if not matches:
                    continue  # Existing spec_docs_parity_test checks command paths.
                current = max(matches, key=lambda c: len(c['path']))
                path = current['path']
                if path not in help_flags:
                    help_text = subprocess.check_output([binary, *path.split('/'), '--help'], text=True)
                    help_flags[path] = set(re.findall(r'(?<!\w)--?[a-zA-Z][\w-]*', help_text))
                commands += 1
                tail = words[1+len(path.split('/')):]
                for offset, word in enumerate(tail):
                    if word in ('--version', '-V') and offset + 1 < len(tail) and re.match(r'^\d+\.\d+', tail[offset + 1]):
                        errors.append(f'{page.relative_to(ROOT)}:{line}: --version prints CLI version; use the command-specific setter')
                    if word in ('|', '>', '&&', ';'):
                        break
                    if re.fullmatch(r'--?[a-zA-Z][\w-]*(?:=.*)?', word) and word.split('=')[0] not in help_flags[path]:
                        errors.append(f'{page.relative_to(ROOT)}:{line}: {path}: unsupported flag {word}')
    if errors:
        raise ValueError('\n'.join(errors))
    print(f'Validated {configs} TOML examples and {commands} CLI examples across {len(pages())} pages')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', help='Path to the release CLI; checks example flags against its help')
    check(parser.parse_args().binary)

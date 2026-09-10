#!/usr/bin/env python3
"""Exercise the published CLI in disposable project directories; no user config writes."""
import argparse
import json
from pathlib import Path
import re
import shlex
import subprocess
import tempfile
import os

ROOT = Path(__file__).resolve().parents[1]


def main(binary, verify_git=False):
    binary = str(Path(binary).resolve())
    child_env = os.environ.copy()
    for key in ('OPENAI_API_KEY', 'OPENAI_BASE_URL', 'FASTSKILL_EMBEDDING_MODEL'):
        child_env.pop(key, None)
    with tempfile.TemporaryDirectory(prefix='fastskill-docs-') as directory:
        temp = Path(directory)
        child_env['FASTSKILL_CACHE_DIR'] = str(temp / 'cache')
        source = (ROOT / 'quickstart.mdx').read_text()
        blocks = re.findall(r'^```bash\n(.*?)^```', source, re.M | re.S)
        script = re.sub(r'(?m)^fastskill\b', shlex.quote(binary), '\n'.join(blocks))
        subprocess.run(['bash', '--noprofile', '--norc', '-eu', '-c', script], cwd=temp, env=child_env, check=True, timeout=90)
        project = temp / 'fastskill-demo'

        def cli(*args, cwd=project, json_output=False):
            result = subprocess.run([binary, *args], cwd=cwd, env=child_env, text=True, capture_output=True, timeout=60)
            if result.returncode:
                raise RuntimeError(f'{args}: {result.stdout}\n{result.stderr}')
            return json.loads(result.stdout) if json_output else result.stdout

        assert (project / '.claude/skills/review-notes/SKILL.md').read_text() == (project / 'source/review-notes/SKILL.md').read_text()
        cli('project', 'install', '--dry-run', '--json', json_output=True)
        cli('skill', 'list', '--check', '--json', json_output=True)
        cli('skill', 'search', 'meeting', '--local', '--embedding', 'false', '--json', json_output=True)
        missing = subprocess.run([binary, 'skill', 'read', 'does-not-exist', '--json'], cwd=project, env=child_env, text=True, capture_output=True)
        assert missing.returncode == 1
        json.loads(missing.stdout)

        manifest = project / 'skill-project.toml'
        manifest.write_text(manifest.read_text() + '\n[bundle]\nformat = "fastskill-bundle-v1"\nid = "notes-team"\nversion = "1.0.0"\n\n[bundle.members.review-notes]\noverridable = true\n')
        cli('bundle', 'build', '--output', 'dist')
        recipient = temp / 'recipient'
        recipient.mkdir()
        cli('project', 'init', '--yes', '--skills-dir', '.claude/skills', cwd=recipient)
        cli('bundle', 'add', str(project / 'dist/notes-team-1.0.0.zip'), cwd=recipient)
        lock = (recipient / 'skills.lock').read_bytes()
        cli('project', 'install', '--lock', '--offline', cwd=recipient)
        assert (recipient / 'skills.lock').read_bytes() == lock
        cli('skill', 'list', '--check', '--json', cwd=recipient, json_output=True)
        cli('bundle', 'list', cwd=recipient)
        cli('mcp', 'install', '--agent', 'claude', '--stdio', '--scope', 'project', '--dry-run', cwd=recipient)
        cli('mcp', 'install', '--agent', 'cursor', '--stdio', '--scope', 'project', '--dry-run', cwd=recipient)
        assert not (recipient / '.mcp.json').exists()
        assert not (recipient / '.cursor/mcp.json').exists()
        if verify_git:
            git_project = temp / 'git-project'
            git_project.mkdir()
            cli('project', 'init', '--yes', '--skills-dir', '.claude/skills', cwd=git_project)
            manifest = git_project / 'skill-project.toml'
            dependency = 'fastskill = { origin = { type = "git", url = "https://github.com/gofastskill/skill.git", subdir = "fastskill", ref = { branch = "main" } } }'
            manifest.write_text(manifest.read_text().replace('[dependencies]', '[dependencies]\n' + dependency))
            cli('project', 'install', '--no-reindex', cwd=git_project)
            before = (git_project / 'skills.lock').read_bytes()
            cli('project', 'install', '--lock', '--offline', cwd=git_project)
            assert (git_project / 'skills.lock').read_bytes() == before
            cli('skill', 'list', '--check', '--json', cwd=git_project, json_output=True)
            print('Passed: Git subdirectory installation and offline locked commit restoration')
        print('Passed: exact quickstart, installed agent skill, JSON success/error, bundle sharing, locked offline restore, MCP registration previews')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('binary')
    parser.add_argument('--git', action='store_true', help='Also verify the public FastSkill Git source; requires network')
    args = parser.parse_args()
    main(args.binary, args.git)

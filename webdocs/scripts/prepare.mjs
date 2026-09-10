import { execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';

// Build metadata comes from this checkout, never a manually maintained label.
const cargo = readFileSync('../Cargo.toml', 'utf8');
const version = cargo.match(/^version = "([^"]+)"/m)?.[1];
if (!version) throw new Error('Missing Cargo workspace version');
const git = (...args) => execFileSync('git', args, { encoding: 'utf8' }).trim();
const tag = `v${version}`;
let sourceRevision;
try { sourceRevision = git('rev-parse', `${tag}^{commit}`); } catch { sourceRevision = null; }
const documentationRevision = git('rev-parse', 'HEAD');
// Docs corrections can be built on top of a release. Product code must still match.
const changed = sourceRevision ? git('diff', '--name-only', tag, '--', '../Cargo.toml', '../Cargo.lock', '../crates', ':(exclude)../crates/*/tests/**') : 'Release tag does not exist';
const status = changed ? 'preview' : 'release';
const documentationDirty = Boolean(git('status', '--porcelain', '--untracked-files=normal', '--', '.'));
if (process.argv.includes('--publish') && documentationDirty) throw new Error('Commit documentation changes before publishing');
if (process.argv.includes('--publish') && status !== 'release') {
  throw new Error(`Product differs from ${tag}; release it before publishing its docs:\n${changed}`);
}
sourceRevision ??= documentationRevision;
mkdirSync('lib/generated', { recursive: true });
writeFileSync('lib/generated/release.json', JSON.stringify({
  version, tag, status, sourceRevision, documentationRevision, documentationDirty,
  baseUrl: 'https://docs.gofastskill.com',
}, null, 2) + '\n');

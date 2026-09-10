#!/usr/bin/env python3
"""Verify release identity and discovery on the production host after deployment."""
import argparse
import json
from urllib.request import urlopen

BASE = 'https://docs.gofastskill.com'


def fetch(base, path):
    with urlopen(base + path, timeout=30) as response:
        if response.status != 200:
            raise RuntimeError(f'{path}: HTTP {response.status}')
        return response.read().decode()


def main(tag, revision, base=BASE):
    manifest = json.loads(fetch(base, '/documentation.json'))
    if manifest['tag'] != tag or manifest['status'] != 'release' or manifest.get('documentationDirty'):
        raise RuntimeError('Public documentation does not match the selected release')
    if manifest['documentationRevision'] != revision:
        raise RuntimeError('Public documentation is from a different source revision')
    index = fetch(base, '/llms.txt')
    full = fetch(base, '/llms-full.txt')
    for page in manifest['pages']:
        if page['markdownUrl'] not in index or page['markdown'] not in full:
            raise RuntimeError(f'Missing export: {page["url"]}')
        if fetch(base, page['markdownUrl']) != page['markdown']:
            raise RuntimeError(f'Stale Markdown: {page["url"]}')
        html = fetch(base, page['url'])
        if manifest['documentationRevision'] not in html or page['markdownUrl'] not in html:
            raise RuntimeError(f'Stale HTML: {page["url"]}')
    for path in ('/api/search', '/sitemap.xml', '/robots.txt'):
        fetch(base, path)
    print(f'Public HTML, Markdown, search and discovery match {tag}: {manifest["documentationRevision"]}')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('tag')
    parser.add_argument('revision', help='Expected documentation Git revision')
    parser.add_argument('--base-url', default=BASE, help='Override host for local static-server verification')
    args = parser.parse_args()
    main(args.tag, args.revision, args.base_url.rstrip('/'))

import assert from 'node:assert/strict';
import { readFile, access } from 'node:fs/promises';
import { resolve } from 'node:path';
import { staticClient } from 'fumadocs-core/search/client/orama-static';

const root = resolve('out');
const errors = [];
const check = (condition, message) => { if (!condition) errors.push(message); };
const read = (path) => readFile(`${root}/${path}`, 'utf8');
const data = JSON.parse(await read('documentation.json'));
const index = await read('llms.txt');
const full = await read('llms-full.txt');
const sitemap = await read('sitemap.xml');
assert(data.pages.length > 40, 'Missing documentation pages');
assert.equal(new Set(data.pages.map((page) => page.url)).size, data.pages.length);
assert.match(data.sourceRevision, /^[a-f0-9]{40}$/);
assert.match(data.documentationRevision, /^[a-f0-9]{40}$/);
const pages = new Map(data.pages.map((page) => [page.url, page]));
const htmlFor = (url) => url === '/' ? 'index.html' : `${url.slice(1)}/index.html`;
const canonicalUrl = (url) => url === '/' ? '/' : `${url}/`;
const decode = (s) => s.replaceAll('&#x27;', "'").replaceAll('&quot;', '"').replaceAll('&amp;', '&');
const htmlByUrl = new Map(await Promise.all(data.pages.map(async (page) => [page.url, await read(htmlFor(page.url))])));
for (const page of data.pages) {
  const html = htmlByUrl.get(page.url);
  assert(index.includes(`${data.baseUrl}${page.markdownUrl}`), `Index omits ${page.url}`);
  assert(sitemap.includes(`${data.baseUrl}${page.url}`), `Sitemap omits ${page.url}`);
  const canonical = html.match(/<link rel="canonical" href="([^"]+)"/);
  check(canonical && new URL(canonical[1]).href === new URL(`${data.baseUrl}${canonicalUrl(page.url)}`).href, `Canonical URL missing: ${page.url}`);
  assert(html.includes(`FastSkill <!-- -->${data.version}`) || html.includes(`FastSkill ${data.version}`), `Version missing: ${page.url}`);
  assert(html.includes(`href="${page.markdownUrl}"`), `Markdown action missing: ${page.url}`);
  const markdown = await read(page.markdownUrl.slice(1));
  assert.equal(markdown, page.markdown);
  assert(full.includes(markdown), `Full export omits ${page.url}`);
  const prose = markdown.replace(/```[\s\S]*?```/g, '');
  assert(!/<\/?(?:Callout|Cards?|Tabs?|Accordions?|Steps?|FastSkillHero)\b/.test(prose), `Unrendered MDX: ${page.url}`);
  for (const [, raw] of html.matchAll(/(?:href|src)="([^"]+)"/g)) {
    if (!raw.startsWith('/') && !raw.startsWith('#')) continue;
    const url = new URL(decode(raw), `${data.baseUrl}${page.url}`);
    if (url.origin !== data.baseUrl) continue;
    if (pages.has(url.pathname)) {
      if (url.hash) {
        const id = decodeURIComponent(url.hash.slice(1));
        check(htmlByUrl.get(url.pathname).includes(`id="${id}"`), `${page.url}: broken anchor ${raw}`);
      }
    } else {
      const pathname = decodeURIComponent(url.pathname);
      // Next emits static extensionless search data; accept its actual export path.
      await access(`${root}${pathname}`).catch(() => {
        errors.push(`${page.url}: missing asset or page ${raw}`);
      });
    }
  }
}
const search = JSON.parse(await read('api/search'));
assert(Object.keys(search).length > 0, 'Empty static search index');
const client = staticClient({ from: `data:application/json;base64,${Buffer.from(JSON.stringify(search)).toString('base64')}` });
const results = await client.search('review-notes');
assert(results.some((result) => result.url.startsWith('/quickstart')), 'Search cannot find the quickstart skill');
assert((await read('robots.txt')).includes(`${data.baseUrl}/sitemap.xml`));
await access(`${root}/.nojekyll`);
assert.equal(errors.length, 0, errors.join('\n'));
console.log(`Validated ${data.pages.length} HTML/Markdown pages, links, anchors, metadata, search, and discovery files`);

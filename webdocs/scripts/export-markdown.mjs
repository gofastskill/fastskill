import { readFile, writeFile, mkdir } from 'node:fs/promises';
import { dirname, resolve, sep } from 'node:path';

const root = resolve('out');
const { pages } = JSON.parse(await readFile('out/documentation.json', 'utf8'));
for (const page of pages) {
  const file = resolve(root, `.${page.markdownUrl}`);
  if (!file.startsWith(`${root}${sep}`) || !file.endsWith('.md')) throw new Error(`Invalid Markdown path: ${file}`);
  await mkdir(dirname(file), { recursive: true });
  await writeFile(file, page.markdown);
}
await writeFile('out/.nojekyll', '');
console.log(`Exported ${pages.length} Markdown pages`);

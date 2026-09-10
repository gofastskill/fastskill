import { source } from '@/lib/source';
import { markdownUrl, release } from '@/lib/release';

export async function exportedPages() {
  return Promise.all(source.getPages().map(async (page) => ({
    title: page.data.title,
    description: page.data.description ?? '',
    url: page.url,
    markdownUrl: markdownUrl(page.url),
    markdown: `# ${page.data.title}\n\nFastSkill ${release.version}${release.status === 'preview' ? ' (unreleased preview)' : ''}\n\nSource: ${release.baseUrl}${page.url}\n\nRelease revision: ${release.sourceRevision}\n\nDocumentation revision: ${release.documentationRevision}${release.documentationDirty ? ' (working copy)' : ''}\n\n${await page.data.getText('processed')}\n`,
  })));
}

export function documentationIndex() {
  return `# FastSkill ${release.version}${release.status === 'preview' ? ' (unreleased preview)' : ''}\n\nDocumentation for the current release only.\n\nRelease revision: ${release.sourceRevision}\nDocumentation revision: ${release.documentationRevision}${release.documentationDirty ? ' (working copy)' : ''}\n\n## Pages\n\n${source.getPages().map((page) =>
    `- [${page.data.title}](${release.baseUrl}${markdownUrl(page.url)}): ${page.data.description ?? ''}`
  ).join('\n')}\n\n## Complete documentation\n\n- [All pages](${release.baseUrl}/llms-full.txt)\n`;
}

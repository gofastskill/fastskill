import release from './generated/release.json';

export { release };
export const repositoryUrl = 'https://github.com/gofastskill/fastskill';
export function markdownUrl(url: string) {
  return url === '/' ? '/index.md' : `${url}.md`;
}

import type { LLMsOptions } from 'fumadocs-core/mdx-plugins/remark-llms';

/** Preserve the useful text of UI components without requiring an MDX renderer. */
export const markdownOptions: LLMsOptions = {
  headingIds: false,
  stringify(node, _parent, state, info) {
    if (node.type !== 'mdxJsxFlowElement' && node.type !== 'mdxJsxTextElement') return;
    const attr = (name: string) => {
      const value = node.attributes.find((a) => a.type === 'mdxJsxAttribute' && a.name === name);
      return value?.type === 'mdxJsxAttribute' && typeof value.value === 'string' ? value.value : '';
    };
    if (node.name === 'FastSkillHero') return 'Install and manage agent skills. [Start with a local skill](/quickstart).\n';
    if (node.name === 'img') return `![${attr('alt')}](${attr('src')})`;
    const children = node.type === 'mdxJsxFlowElement'
      ? state.containerFlow(node, info)
      : state.containerPhrasing(node, info);
    const label = attr('title') || (node.name === 'Tab' ? attr('value') : '');
    const href = attr('href');
    if (href) return `[${label || children || href}](${href})${label && children ? `\n\n${children}` : ''}\n`;
    return `${label ? `**${label}**\n\n` : ''}${children}\n`;
  },
};

import type { BaseLayoutProps } from 'fumadocs-ui/layouts/shared';
import { FastSkillBrand } from '@/components/brand';

export function baseOptions(): BaseLayoutProps {
  return {
    nav: {
      title: <FastSkillBrand />,
      url: '/',
    },
    links: [
      {
        text: 'Quick start',
        url: '/quickstart',
        active: 'nested-url',
      },
      {
        text: 'For agents',
        url: '/integration/agents',
        active: 'url',
      },
    ],
    githubUrl: 'https://github.com/gofastskill/fastskill',
  };
}

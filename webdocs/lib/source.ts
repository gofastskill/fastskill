import { markdownOptions } from './markdown-options';
import { loader } from 'fumadocs-core/source';
import { metaSchema, pageSchema } from 'fumadocs-core/source/schema';
import { remarkSteps } from 'fumadocs-core/mdx-plugins';
import { applyMdxPreset } from 'fumadocs-mdx/config';
import { defineDocs } from 'fumadocs-mdx/macro';

const docs = defineDocs({
  dir: '.',
  docs: {
    files: [
      '*.mdx',
      'cli-reference/*.mdx',
      'configuration/*.mdx',
      'evals-quality/*.mdx',
      'examples/*.mdx',
      'integration/*.mdx',
      'optimize/*.mdx',
      'progressive-loading/*.mdx',
      'registry/*.mdx',
      'security/*.mdx',
      'skill-management/*.mdx',
      'testing/*.mdx',
      'tool-calling/*.mdx',
    ],
    mdxOptions: applyMdxPreset({
      remarkPlugins: (plugins) => [...plugins, remarkSteps],
    }),
    postprocess: {
      includeProcessedMarkdown: markdownOptions,
    },
    schema: pageSchema,
  },
  meta: {
    files: [
      'meta.json',
      'cli-reference/meta.json',
      'configuration/meta.json',
      'evals-quality/meta.json',
      'examples/meta.json',
      'integration/meta.json',
      'optimize/meta.json',
      'progressive-loading/meta.json',
      'registry/meta.json',
      'security/meta.json',
      'skill-management/meta.json',
      'testing/meta.json',
      'tool-calling/meta.json',
    ],
    schema: metaSchema,
  },
});

export const source = loader({
  baseUrl: '/',
  source: docs.toFumadocsSource(),
});

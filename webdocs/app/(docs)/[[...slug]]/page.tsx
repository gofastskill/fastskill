import { MarkdownActions } from '@/components/markdown-actions';
import { release, repositoryUrl, markdownUrl } from '@/lib/release';
import type { Metadata } from 'next';
import { notFound } from 'next/navigation';
import { createRelativeLink } from 'fumadocs-ui/mdx';
import {
  DocsBody,
  DocsDescription,
  DocsPage,
  DocsTitle,
} from 'fumadocs-ui/layouts/docs/page';
import { getMDXComponents } from '@/components/mdx';
import { source } from '@/lib/source';

type PageProps = {
  params: Promise<{ slug?: string[] }>;
};

export default async function DocumentationPage({ params }: PageProps) {
  const { slug } = await params;
  const page = source.getPage(slug);
  if (!page) notFound();

  const Content = page.data.body;
  const isHome = !slug || slug.length === 0;

  return (
    <DocsPage toc={page.data.toc} full={page.data.full}>
      {!isHome && <DocsTitle>{page.data.title}</DocsTitle>}
      {!isHome && <DocsDescription>{page.data.description}</DocsDescription>}
      <p className="docs-release">
        FastSkill {release.version}{release.status === 'preview' ? ' (unreleased preview)' : ''} · <a href={`${repositoryUrl}/releases/tag/${release.tag}`}>Release</a>
        {' · '}<a href={`${repositoryUrl}/tree/${release.documentationRevision}/webdocs`}>{release.documentationDirty ? 'Documentation source (working copy)' : 'Documentation source'}</a>
      </p>
      <MarkdownActions url={markdownUrl(page.url)} />
      <DocsBody>
        <Content
          components={getMDXComponents({
            a: createRelativeLink(source, page),
          })}
        />
      </DocsBody>
    </DocsPage>
  );
}

export function generateStaticParams() {
  return source.generateParams();
}

export async function generateMetadata({ params }: PageProps): Promise<Metadata> {
  const { slug } = await params;
  const page = source.getPage(slug);
  if (!page) notFound();

  return {
    title: page.data.title,
    description: page.data.description,
    alternates: { canonical: `${release.baseUrl}${page.url}` },
  };
}

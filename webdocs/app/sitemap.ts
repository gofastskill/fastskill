import type { MetadataRoute } from 'next';
import { source } from '@/lib/source';
import { release } from '@/lib/release';

export const dynamic = 'force-static';
export default function sitemap(): MetadataRoute.Sitemap {
  return source.getPages().map((page) => ({ url: `${release.baseUrl}${page.url}` }));
}

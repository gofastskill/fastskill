import type { MetadataRoute } from 'next';
import { release } from '@/lib/release';

export const dynamic = 'force-static';
export default function robots(): MetadataRoute.Robots {
  return { rules: { userAgent: '*', allow: '/' }, sitemap: `${release.baseUrl}/sitemap.xml` };
}

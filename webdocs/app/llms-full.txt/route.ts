import { exportedPages } from '@/lib/exports';

export const dynamic = 'force-static';
export async function GET() {
  return new Response((await exportedPages()).map((page) => page.markdown).join('\n\n---\n\n'), {
    headers: { 'Content-Type': 'text/plain; charset=utf-8' },
  });
}

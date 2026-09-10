import { exportedPages } from '@/lib/exports';
import { release } from '@/lib/release';

export const dynamic = 'force-static';
export async function GET() {
  return Response.json({ ...release, pages: await exportedPages() });
}

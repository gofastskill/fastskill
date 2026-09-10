import { documentationIndex } from '@/lib/exports';

export const dynamic = 'force-static';
export function GET() {
  return new Response(documentationIndex(), { headers: { 'Content-Type': 'text/plain; charset=utf-8' } });
}

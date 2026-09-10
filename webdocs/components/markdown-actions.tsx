'use client';

import { useState } from 'react';

export function MarkdownActions({ url }: { url: string }) {
  const [status, setStatus] = useState('');
  async function copy() {
    try {
      const response = await fetch(url);
      if (!response.ok) throw new Error('Markdown unavailable');
      await navigator.clipboard.writeText(await response.text());
      setStatus('Copied');
    } catch {
      setStatus('Could not copy. Use View Markdown.');
    }
  }
  return (
    <div className="docs-markdown-actions">
      <a href={url}>View Markdown</a>
      <button type="button" onClick={copy}>Copy Markdown</button>
      <span role="status" aria-live="polite">{status}</span>
    </div>
  );
}

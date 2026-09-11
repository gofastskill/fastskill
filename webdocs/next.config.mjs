import { createMDX } from 'fumadocs-mdx/next';

const withMDX = createMDX();

/** @type {import('next').NextConfig} */
const config = {
  output: 'export',
  // The cluster static host resolves /guide to /guide/index.html.
  trailingSlash: true,
  reactStrictMode: true,
};

export default withMDX(config);

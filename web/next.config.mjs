/** @type {import('next').NextConfig} */
const nextConfig = {
  reactStrictMode: true,
  // `next build` emits a self-contained server for the Docker image.
  output: "standalone",
  // The gateway is an internal service; the browser only ever talks to Next.
  async rewrites() {
    return [];
  },
};

export default nextConfig;

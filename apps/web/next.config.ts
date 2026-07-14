import type { NextConfig } from "next";
import "@workspace/env/web";

const nextConfig: NextConfig = {
  transpilePackages: [
    "@workspace/ui",
    "@workspace/api",
    "@workspace/env",
    "@workspace/core",
    "@workspace/viz",
  ],
  reactCompiler: true,
  typedRoutes: true,
  experimental: {
    // Enable filesystem caching for `next dev`
    turbopackFileSystemCacheForDev: true,
    // Enable filesystem caching for `next build`
    turbopackFileSystemCacheForBuild: true,
  },
};

export default nextConfig;

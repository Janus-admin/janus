import type { NextConfig } from "next";

const nextConfig: NextConfig = {
  output: "export",
  // Required for static export — disables Next.js image optimization.
  images: { unoptimized: true },
  // Trailing slash so every page is a directory with index.html,
  // which matches how Rust's static file serving resolves paths.
  trailingSlash: true,
};

export default nextConfig;

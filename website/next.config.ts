import type { NextConfig } from "next";

const nextConfig: NextConfig = {
  reactStrictMode: true,
  async redirects() {
    return [
      {
        source: "/docs/introduction/overview",
        destination: "/docs/introduction",
        permanent: true
      },
      {
        source: "/docs/migration/nestjs-to-nestrs",
        destination: "/docs/nestjs-migration",
        permanent: true
      },
      {
        source: "/docs/introduction/first-steps",
        destination: "/docs/first-steps",
        permanent: true
      }
    ];
  }
};

export default nextConfig;

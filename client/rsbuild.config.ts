import { defineConfig } from '@rsbuild/core';
import { pluginReact } from '@rsbuild/plugin-react';

export default defineConfig({
  plugins: [pluginReact()],
  source: {
    entry: {
      index: './src/main.tsx',
    },
    alias: {
      '@': './src',
    },
  },
  server: {
    port: 5173,
    proxy: {
      '/api/': {
        target: 'http://localhost:3000',
        pathRewrite: { '^/api': '' },
      },
      '/scalar': {
        target: 'http://localhost:3000',
      },
    },
  },
  output: {
    distPath: {
      root: 'dist',
    },
  },
  html: {
    template: './index.html',
  },
  tools: {
    postcss: {
      postcssOptions: {
        plugins: [require('postcss-preset-mantine')],
      },
    },
  },
});

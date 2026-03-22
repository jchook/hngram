import { defineConfig } from '@kubb/core';
import { pluginOas } from '@kubb/plugin-oas';
import { pluginTs } from '@kubb/plugin-ts';
import { pluginReactQuery } from '@kubb/plugin-react-query';

export default defineConfig({
  root: '.',
  input: {
    path: '../server/openapi.json',
  },
  output: {
    path: './src/gen',
    clean: true,
  },
  plugins: [
    pluginOas(),
    pluginTs({
      output: {
        path: 'types',
      },
    }),
    pluginReactQuery({
      output: {
        path: 'hooks',
      },
      client: {
        importPath: '@/lib/client',
      },
      query: {
        methods: ['get'],
      },
    }),
  ],
});

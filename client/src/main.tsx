import React from 'react';
import ReactDOM from 'react-dom/client';
import { createTheme, MantineProvider } from '@mantine/core';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import App from './App';
import { ApiError } from './lib/client';

import '@mantine/core/styles.css';
import '@mantine/dates/styles.css';
import './index.css';
import { phrasePillPaletteCss } from './features/chart/colors';

// Generate phrase-pill colors from the chart palette at startup so
// SERIES_COLORS stays the single source of truth.
const paletteStyle = document.createElement('style');
paletteStyle.dataset.source = 'series-colors';
paletteStyle.textContent = phrasePillPaletteCss();
document.head.appendChild(paletteStyle);

const theme = createTheme({
  fontFamily: 'Verdana, Geneva, sans-serif',
  fontFamilyMonospace: 'monospace',
  headings: { fontFamily: 'Verdana, Geneva, sans-serif' },
  // Black-ink chrome to match news.ycombinator.com — drives input focus
  // border, default button color, and all other "primary" accents. Orange
  // is reserved for explicit brand bits (HN header, feedback "Yes" link)
  // and for chart series (see features/chart/colors.ts).
  primaryColor: 'dark',
  defaultRadius: 0,
});

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 1000 * 60 * 5, // 5 minutes
      // Retry transient failures (5xx, network) but not user errors (4xx).
      // ClickHouse can briefly reject queries under load — wait it out
      // instead of surfacing a noisy error.
      retry: (failureCount, error) => {
        if (error instanceof ApiError && error.status >= 400 && error.status < 500) {
          return false;
        }
        return failureCount < 3;
      },
      // Exponential backoff: 500ms, 1s, 2s (capped at 5s).
      retryDelay: attemptIndex => Math.min(500 * 2 ** attemptIndex, 5000),
    },
  },
});

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <QueryClientProvider client={queryClient}>
      <MantineProvider theme={theme}>
        <App />
      </MantineProvider>
    </QueryClientProvider>
  </React.StrictMode>
);

import { useMemo } from 'react';
import { Container, Paper, Stack, Text } from '@mantine/core';
import { useQueries } from '@tanstack/react-query';
import { ngramQueryOptions, useFreshness } from '@/gen';
import type { NgramQueryResponse } from '@/gen';
import { useQueryState } from '@/features/query/useQueryState';
import { QueryControls } from '@/features/query/QueryControls';
import { TimeSeriesChart } from '@/features/chart/TimeSeriesChart';
import { QueryStatus } from '@/components/QueryStatus';
import {
  fillMissingBuckets,
  applySmoothing,
  buildChartOption,
  type ChartSeries,
} from '@/features/chart/transforms';

export default function App() {
  const { state, setQuery } = useQueryState();
  const { data: freshness } = useFreshness({
    query: { staleTime: 1000 * 60 * 60 },
  });

  // One TanStack query per phrase, parallel
  const results = useQueries({
    queries: state.phrases.map(phrase =>
      ngramQueryOptions({
        phrase,
        start: state.start,
        end: state.end,
        granularity: state.granularity,
      })
    ),
  });

  const isLoading = results.some(r => r.isLoading);

  // Transform: sparse → zero-fill → smooth → chart series
  const chartSeries = useMemo<ChartSeries[]>(() => {
    const series: ChartSeries[] = [];

    for (let i = 0; i < state.phrases.length; i++) {
      const result = results[i];
      if (!result?.data) continue;

      const data = result.data as NgramQueryResponse;
      if (data.status !== 'indexed' || data.points.length === 0) continue;

      const gran = (data.meta.granularity || state.granularity) as
        'day' | 'week' | 'month' | 'year';

      const filled = fillMissingBuckets(
        data.points,
        data.meta.start,
        data.meta.end,
        gran,
      );
      const smoothed = applySmoothing(filled, state.smoothing);

      series.push({
        label: state.phrases[i],
        points: smoothed,
        globalCount: data.global_count,
      });
    }

    return series;
  }, [results, state.phrases, state.smoothing, state.granularity]);

  const chartOption = useMemo(
    () => buildChartOption(chartSeries),
    [chartSeries],
  );

  const hasData = chartSeries.length > 0;
  const allDone = results.every(r => !r.isLoading);

  return (
    <>
      <header className="hn-header">
        <a href="/" className="hn-logo" aria-label="HN N-gram home">
          <img src="/y18.svg" alt="" width={18} height={18} />
        </a>
        <a href="/" className="hn-name">HN N-gram</a>
        <span className="hn-tagline">
          Explore word and phrase trends in Hacker News comments over time
        </span>
      </header>
      <Container size="lg" py="md">
        <Stack gap="md">
        <Paper p="md" withBorder>
          <QueryControls state={state} onSubmit={setQuery} />
        </Paper>

        <QueryStatus phrases={state.phrases} results={results as never[]} />

        <Paper p="md" withBorder style={{ height: 'clamp(400px, 50vw, 520px)' }}>
          {hasData ? (
            <TimeSeriesChart option={chartOption} loading={isLoading} />
          ) : allDone && state.phrases.length > 0 ? (
            <Text c="dimmed" ta="center" py="xl">
              No data found for the selected phrases and date range
            </Text>
          ) : isLoading ? (
            <TimeSeriesChart option={chartOption} loading />
          ) : null}
        </Paper>
        </Stack>
      </Container>
      <footer className="hn-footer">
        <nav className="hn-footer-links">
          <a href="https://github.com/jchook/hngram" target="_blank" rel="noopener noreferrer">GitHub</a>
          <span aria-hidden="true">|</span>
          <a href="/scalar" target="_blank" rel="noopener noreferrer">API</a>
          <span aria-hidden="true">|</span>
          <a href="https://huggingface.co/datasets/open-index/hacker-news" target="_blank" rel="noopener noreferrer">Dataset</a>
          <span aria-hidden="true">|</span>
          <a href="https://github.com/jchook/hngram/blob/main/docs/RFC-001-tokenization.md" target="_blank" rel="noopener noreferrer">Methodology</a>
        </nav>
        {freshness?.last_ingested_date && (
          <div className="hn-footer-meta">Updated through {freshness.last_ingested_date}</div>
        )}
      </footer>
    </>
  );
}

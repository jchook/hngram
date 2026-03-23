import { useMemo } from 'react';
import { Container, Paper, Stack, Text, Title } from '@mantine/core';
import { useQueries } from '@tanstack/react-query';
import { ngramQueryOptions } from '@/gen';
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
    <Container size="lg" py="xl">
      <Stack gap="lg">
        <div>
          <Title order={1}>HN N-gram Viewer</Title>
          <Text c="dimmed">
            Explore word and phrase trends in Hacker News comments over time
          </Text>
        </div>

        <Paper p="md" withBorder>
          <QueryControls state={state} onSubmit={setQuery} />
        </Paper>

        <QueryStatus phrases={state.phrases} results={results as never[]} />

        <Paper p="md" withBorder>
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
  );
}

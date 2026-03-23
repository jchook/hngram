import ReactECharts from 'echarts-for-react';
import { Skeleton } from '@mantine/core';
import type { EChartsOption } from 'echarts';

interface TimeSeriesChartProps {
  option: EChartsOption;
  loading?: boolean;
}

export function TimeSeriesChart({ option, loading }: TimeSeriesChartProps) {
  if (loading) {
    return <Skeleton height={400} />;
  }

  return (
    <ReactECharts
      option={option}
      style={{ height: 400 }}
      notMerge
    />
  );
}

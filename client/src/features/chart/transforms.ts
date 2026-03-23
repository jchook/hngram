/**
 * Data transformation pipeline: sparse → zero-fill → smooth → ECharts.
 */

import dayjs from 'dayjs';
import isoWeek from 'dayjs/plugin/isoWeek';
import type { Point } from '@/gen';
import type { EChartsOption } from 'echarts';

dayjs.extend(isoWeek);

// ============================================================================
// Zero-fill
// ============================================================================

type Granularity = 'day' | 'week' | 'month' | 'year';

/**
 * Fill missing buckets with v=0 to produce a continuous series.
 */
export function fillMissingBuckets(
  points: Point[],
  start: string,
  end: string,
  granularity: Granularity,
): Point[] {
  // Build lookup from sparse data
  const lookup = new Map<string, number>();
  for (const p of points) {
    lookup.set(p.t, p.v);
  }

  const result: Point[] = [];
  let current = alignToBucket(dayjs(start), granularity);
  const endDate = dayjs(end);

  while (current.isBefore(endDate) || current.isSame(endDate, 'day')) {
    const key = current.format('YYYY-MM-DD');
    result.push({ t: key, v: lookup.get(key) ?? 0 });
    current = advanceBucket(current, granularity);
  }

  return result;
}

function alignToBucket(d: dayjs.Dayjs, g: Granularity): dayjs.Dayjs {
  switch (g) {
    case 'day': return d;
    case 'week': return d.startOf('isoWeek');
    case 'month': return d.startOf('month');
    case 'year': return d.startOf('year');
  }
}

function advanceBucket(d: dayjs.Dayjs, g: Granularity): dayjs.Dayjs {
  switch (g) {
    case 'day': return d.add(1, 'day');
    case 'week': return d.add(1, 'week');
    case 'month': return d.add(1, 'month');
    case 'year': return d.add(1, 'year');
  }
}

// ============================================================================
// Smoothing
// ============================================================================

/**
 * Centered simple moving average (RFC-006 §12).
 */
export function applySmoothing(points: Point[], window: number): Point[] {
  if (window <= 1) return points;
  return points.map((point, i) => {
    const start = Math.max(0, i - Math.floor(window / 2));
    const end = Math.min(points.length, i + Math.ceil(window / 2));
    const slice = points.slice(start, end);
    const avg = slice.reduce((sum, p) => sum + p.v, 0) / slice.length;
    return { t: point.t, v: avg };
  });
}

// ============================================================================
// ECharts option builder
// ============================================================================

export interface ChartSeries {
  label: string;
  points: Point[];
}

const COLORS = [
  '#5470c6', '#91cc75', '#fac858', '#ee6666', '#73c0de',
  '#3ba272', '#fc8452', '#9a60b4', '#ea7ccc', '#48b8d0',
];

/**
 * Build a complete ECharts option from transformed series data.
 */
export function buildChartOption(series: ChartSeries[]): EChartsOption {
  return {
    tooltip: {
      trigger: 'axis',
      valueFormatter: (value) => formatFrequency(value as number),
    },
    legend: {
      show: series.length > 1,
      bottom: 0,
    },
    grid: {
      left: 80,
      right: 20,
      top: 20,
      bottom: series.length > 1 ? 40 : 10,
    },
    xAxis: {
      type: 'time',
    },
    yAxis: {
      type: 'value',
      axisLabel: {
        formatter: (value: number) => formatFrequency(value),
      },
    },
    dataZoom: [
      { type: 'inside' },
      { type: 'slider', height: 20, bottom: series.length > 1 ? 30 : 0 },
    ],
    color: COLORS,
    series: series.map((s) => ({
      name: s.label,
      type: 'line' as const,
      showSymbol: false,
      data: s.points.map(p => [p.t, p.v]),
    })),
  };
}

function formatFrequency(v: number): string {
  if (v === 0) return '0';
  if (v < 0.0001) return v.toExponential(2);
  if (v < 0.01) return v.toFixed(5);
  return v.toFixed(4);
}

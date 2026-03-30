/**
 * Data transformation pipeline: sparse → zero-fill → smooth → ECharts.
 */

import dayjs from 'dayjs';
import type { Point } from '@/gen';
import type { EChartsOption } from 'echarts';

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
  const lookup = new Map<string, Point>();
  for (const p of points) {
    lookup.set(p.t, p);
  }

  const result: Point[] = [];
  let current = alignToBucket(dayjs(start), granularity);
  const endDate = dayjs(end);

  while (current.isBefore(endDate) || current.isSame(endDate, 'day')) {
    const key = current.format('YYYY-MM-DD');
    const existing = lookup.get(key);
    result.push(existing ?? { t: key, v: 0, count: 0, total: 0 });
    current = advanceBucket(current, granularity);
  }

  return result;
}

function alignToBucket(d: dayjs.Dayjs, g: Granularity): dayjs.Dayjs {
  switch (g) {
    case 'day': return d;
    case 'week': return d.startOf('week');
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
    return { t: point.t, v: avg, count: point.count, total: point.total };
  });
}

// ============================================================================
// ECharts option builder
// ============================================================================

export interface ChartSeries {
  label: string;
  points: Point[];
  globalCount: number;
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
      formatter: (params: unknown) => {
        const items = params as Array<{
          seriesName: string;
          color: string;
          data: [string, number, number, number];
        }>;
        if (!items?.length) return '';
        const date = items[0].data[0];
        const lines = items.map(item => {
          const [, v, count, total] = item.data;
          const freq = formatFrequency(v);
          const counts = count > 0 || total > 0 ? ` <span style="color:#999">(${formatCount(count)} / ${formatCount(total)})</span>` : '';
          return `<span style="display:inline-block;width:10px;height:10px;border-radius:50%;background:${item.color};margin-right:4px;"></span>${item.seriesName}: ${freq}${counts}`;
        });
        return `<strong>${date}</strong><br/>${lines.join('<br/>')}`;
      },
    },
    legend: {
      show: true,
      bottom: 0,
      formatter: (name: string) => {
        const s = series.find(s => s.label === name);
        if (s && s.globalCount > 0) return `${name} (${formatCount(s.globalCount)} total)`;
        return name;
      },
    },
    grid: {
      containLabel: true,
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
      { type: 'slider' },
    ],
    color: COLORS,
    series: series.map((s) => ({
      name: s.label,
      type: 'line' as const,
      showSymbol: false,
      data: s.points.map(p => [p.t, p.v, p.count, p.total]),
    })),
  };
}

function formatCount(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return n.toString();
}

function formatFrequency(v: number): string {
  if (v === 0) return '0';
  if (v < 0.0001) return v.toExponential(2);
  if (v < 0.01) return v.toFixed(5);
  return v.toFixed(4);
}

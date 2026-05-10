/**
 * Palette for chart series and the matching phrase pills in QueryControls.
 * Single source of truth — both consumers read from here so the nth chart
 * line and the nth phrase pill always share a color.
 *
 * Orange is intentionally last; the page already uses orange for the HN
 * banner, primary button, and feedback link, so leading the palette with
 * it made the chart feel monochromatic.
 */
export const SERIES_COLORS = [
  '#1f77b4', // blue
  '#2ca02c', // green
  '#9467bd', // purple
  '#d62728', // red
  '#17becf', // cyan
  '#8c564b', // brown
  '#bcbd22', // yellow-green
  '#e377c2', // pink
  '#7f7f7f', // gray
  '#ff6600', // orange
];

/**
 * Pick a text color (white or near-black) that's readable on the given
 * background. Uses the YIQ luminance formula — the threshold is tuned so
 * only very light backgrounds (e.g. yellow-green) flip to dark text.
 */
export function readableTextOn(bg: string): string {
  const n = parseInt(bg.replace('#', ''), 16);
  const r = (n >> 16) & 0xff;
  const g = (n >> 8) & 0xff;
  const b = n & 0xff;
  const yiq = (r * 299 + g * 587 + b * 114) / 1000;
  return yiq >= 165 ? '#222' : '#fff';
}

/**
 * Generate the per-pill background/text-color CSS rules for `.phrases-input`.
 * Used by main.tsx at app startup so QueryControls' pills automatically
 * track SERIES_COLORS without manually duplicating values in CSS.
 */
export function phrasePillPaletteCss(): string {
  return SERIES_COLORS.map((bg, i) => {
    const fg = readableTextOn(bg);
    const slot = i + 1;
    const len = SERIES_COLORS.length;
    return (
      `.phrases-input .mantine-Pill-root:nth-of-type(${len}n+${slot}){` +
      `background-color:${bg};color:${fg};}`
    );
  }).join('\n');
}

/**
 * URL state management for the n-gram viewer.
 *
 * All query state lives in the URL via URLSearchParams.
 * Params: q (phrases), since (start year preset), g (granularity), s (smoothing), y (y-axis scale).
 */

import { useCallback, useEffect, useState } from 'react';
import launchDefaults from '../../../../config/launch-defaults.json';
import suggested from '../../../../config/suggested-comparisons.json';

// Defaults are sourced from config/launch-defaults.json so the deploy
// smoke test (.github/workflows/deploy.yml) and the frontend never drift
// out of sync — the test pre-warms exactly what visitors land on.
const DEFAULT_GRANULARITY = launchDefaults.granularity;
const DEFAULT_SMOOTHING = launchDefaults.smoothing;
const DEFAULT_PHRASES = launchDefaults.phrases;
const DEFAULT_SINCE = launchDefaults.since as Since;

function phraseSetKey(phrases: string[]): string {
  return phrases.map(p => p.trim().toLowerCase()).sort().join('|');
}

// Default + curated suggestions. We don't ask for feedback on these, since
// the user didn't construct the comparison themselves.
const SUGGESTED_PHRASE_KEYS = new Set<string>(
  [DEFAULT_PHRASES, ...(suggested.comparisons as string[][])].map(phraseSetKey),
);

export function isSuggestedPhraseSet(phrases: string[]): boolean {
  return SUGGESTED_PHRASE_KEYS.has(phraseSetKey(phrases));
}

// Two-value preset for the start year. 2011 is when HN really took off
// (mainstream visibility, comment volume); 2006 is the dataset's beginning.
// Constraining to two values collapses cache cardinality to 2 entries per
// phrase per granularity instead of an unbounded month × month grid.
export type Since = '2006' | '2011';
export const SINCE_OPTIONS: Since[] = ['2006', '2011'];

export function sinceToStart(since: Since): string {
  return since === '2006' ? '2006-01-01' : '2011-01-01';
}

export type YScale = 'linear' | 'log';
export const Y_SCALE_OPTIONS: YScale[] = ['linear', 'log'];
const DEFAULT_Y_SCALE: YScale = 'linear';

export interface QueryState {
  phrases: string[];
  since: Since;
  granularity: string;
  smoothing: number;
  yScale: YScale;
}

function parseFromUrl(): QueryState {
  const params = new URLSearchParams(window.location.search);

  const q = params.get('q');
  const phrases = q
    ? q.split(',').map(s => s.trim()).filter(Boolean)
    : DEFAULT_PHRASES;

  const sinceParam = params.get('since');
  const since: Since = sinceParam === '2006' ? '2006' : DEFAULT_SINCE;

  const yParam = params.get('y');
  const yScale: YScale = yParam === 'log' ? 'log' : DEFAULT_Y_SCALE;

  return {
    phrases,
    since,
    granularity: params.get('g') || DEFAULT_GRANULARITY,
    smoothing: parseInt(params.get('s') || '', 10) || DEFAULT_SMOOTHING,
    yScale,
  };
}

function serializeToUrl(state: QueryState): string {
  const params = new URLSearchParams();
  if (state.phrases.length > 0) {
    params.set('q', state.phrases.join(','));
  }
  if (state.since !== DEFAULT_SINCE) {
    params.set('since', state.since);
  }
  if (state.granularity && state.granularity !== DEFAULT_GRANULARITY) {
    params.set('g', state.granularity);
  }
  if (state.smoothing !== DEFAULT_SMOOTHING) {
    params.set('s', String(state.smoothing));
  }
  if (state.yScale !== DEFAULT_Y_SCALE) {
    params.set('y', state.yScale);
  }
  const qs = params.toString();
  return qs ? `?${qs}` : window.location.pathname;
}

export function useQueryState() {
  const [state, setState] = useState<QueryState>(parseFromUrl);

  // Listen for back/forward navigation
  useEffect(() => {
    const onPopState = () => setState(parseFromUrl());
    window.addEventListener('popstate', onPopState);
    return () => window.removeEventListener('popstate', onPopState);
  }, []);

  const setQuery = useCallback((next: Partial<QueryState>) => {
    setState(prev => {
      const merged = { ...prev, ...next };
      const url = serializeToUrl(merged);
      history.pushState(null, '', url);
      return merged;
    });
  }, []);

  return { state, setQuery };
}

/**
 * URL state management for the n-gram viewer.
 *
 * All query state lives in the URL via URLSearchParams.
 * Params: q (phrases), start, end, g (granularity), s (smoothing).
 */

import { useCallback, useEffect, useState } from 'react';
import launchDefaults from '../../../../config/launch-defaults.json';

// Defaults are sourced from config/launch-defaults.json so the deploy
// smoke test (.github/workflows/deploy.yml) and the frontend never drift
// out of sync — the test pre-warms exactly what visitors land on.
const DEFAULT_START = launchDefaults.start;
const DEFAULT_GRANULARITY = launchDefaults.granularity;
const DEFAULT_SMOOTHING = launchDefaults.smoothing;
const DEFAULT_PHRASES = launchDefaults.phrases;

export interface QueryState {
  phrases: string[];
  start: string;
  end: string;
  granularity: string;
  smoothing: number;
}

function todayString(): string {
  return new Date().toISOString().slice(0, 10);
}

function parseFromUrl(): QueryState {
  const params = new URLSearchParams(window.location.search);

  const q = params.get('q');
  const phrases = q
    ? q.split(',').map(s => s.trim()).filter(Boolean)
    : DEFAULT_PHRASES;

  return {
    phrases,
    start: params.get('start') || DEFAULT_START,
    end: params.get('end') || todayString(),
    granularity: params.get('g') || DEFAULT_GRANULARITY,
    smoothing: parseInt(params.get('s') || '', 10) || DEFAULT_SMOOTHING,
  };
}

function serializeToUrl(state: QueryState): string {
  const params = new URLSearchParams();
  if (state.phrases.length > 0) {
    params.set('q', state.phrases.join(','));
  }
  if (state.start && state.start !== DEFAULT_START) {
    params.set('start', state.start);
  }
  if (state.end && state.end !== todayString()) {
    params.set('end', state.end);
  }
  if (state.granularity && state.granularity !== DEFAULT_GRANULARITY) {
    params.set('g', state.granularity);
  }
  if (state.smoothing !== DEFAULT_SMOOTHING) {
    params.set('s', String(state.smoothing));
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

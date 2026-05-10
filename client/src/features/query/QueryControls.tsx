import { useState, useEffect } from 'react';
import { Button, Group, NumberInput, SegmentedControl, Stack, TextInput } from '@mantine/core';
import type { QueryState, Since } from './useQueryState';
import { SINCE_OPTIONS } from './useQueryState';

interface QueryControlsProps {
  state: QueryState;
  onSubmit: (next: Partial<QueryState>) => void;
}

export function QueryControls({ state, onSubmit }: QueryControlsProps) {
  // Local ephemeral state for inputs (committed on submit)
  const [phrases, setPhrases] = useState(state.phrases.join(', '));
  const [since, setSince] = useState<Since>(state.since);
  const [smoothing, setSmoothing] = useState(state.smoothing);

  // Sync from external state changes (e.g. popstate)
  useEffect(() => {
    setPhrases(state.phrases.join(', '));
    setSince(state.since);
    setSmoothing(state.smoothing);
  }, [state]);

  const handleSubmit = () => {
    const parsed = phrases
      .split(',')
      .map(s => s.trim())
      .filter(Boolean)
      .slice(0, 10);

    if (parsed.length === 0) return;

    onSubmit({
      phrases: parsed,
      since,
      smoothing,
    });
  };

  return (
    <form onSubmit={e => { e.preventDefault(); handleSubmit(); }}>
    <Stack gap="sm">
      <TextInput
        label="Phrases"
        placeholder="rust, go, python"
        description="Comma-separated phrases (max 10)"
        value={phrases}
        onChange={e => setPhrases(e.currentTarget.value)}
      />
      <Group grow align="end">
        <SegmentedControl
          value={since}
          onChange={v => setSince(v as Since)}
          data={SINCE_OPTIONS.map(s => ({ label: `Since ${s}`, value: s }))}
          fullWidth
        />
        <NumberInput
          label="Smoothing"
          min={0}
          max={12}
          step={1}
          value={smoothing}
          onChange={v => setSmoothing(typeof v === 'number' ? v : parseInt(v, 10) || 0)}
          clampBehavior="strict"
        />
      </Group>
      <Group justify="center" pt="md" pb="xs">
        <Button type="submit" size="md" px="xl" style={{ flex: 1, maxWidth: 360 }}>Show Trends</Button>
      </Group>
    </Stack>
    </form>
  );
}

import { useState, useEffect } from 'react';
import { Button, Group, NumberInput, Stack, TextInput } from '@mantine/core';
import { DateInput } from '@mantine/dates';
import dayjs from 'dayjs';
import type { QueryState } from './useQueryState';

interface QueryControlsProps {
  state: QueryState;
  onSubmit: (next: Partial<QueryState>) => void;
}

export function QueryControls({ state, onSubmit }: QueryControlsProps) {
  // Local ephemeral state for inputs (committed on submit)
  const [phrases, setPhrases] = useState(state.phrases.join(', '));
  const [start, setStart] = useState<Date | null>(dayjs(state.start).toDate());
  const [end, setEnd] = useState<Date | null>(dayjs(state.end).toDate());
  const [smoothing, setSmoothing] = useState(state.smoothing);

  // Sync from external state changes (e.g. popstate)
  useEffect(() => {
    setPhrases(state.phrases.join(', '));
    setStart(dayjs(state.start).toDate());
    setEnd(dayjs(state.end).toDate());
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
      start: start ? dayjs(start).format('YYYY-MM-DD') : state.start,
      end: end ? dayjs(end).format('YYYY-MM-DD') : state.end,
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
      <Group grow>
        <DateInput
          label="Start"
          value={start}
          onChange={setStart}
          valueFormat="YYYY-MM-DD"
          maxDate={end || undefined}
        />
        <DateInput
          label="End"
          value={end}
          onChange={setEnd}
          valueFormat="YYYY-MM-DD"
          minDate={start || undefined}
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

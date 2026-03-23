import { useState, useEffect } from 'react';
import { Button, Group, Select, Slider, Stack, TextInput } from '@mantine/core';
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
  const [granularity, setGranularity] = useState(state.granularity);
  const [smoothing, setSmoothing] = useState(state.smoothing);

  // Sync from external state changes (e.g. popstate)
  useEffect(() => {
    setPhrases(state.phrases.join(', '));
    setStart(dayjs(state.start).toDate());
    setEnd(dayjs(state.end).toDate());
    setGranularity(state.granularity);
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
      granularity,
      smoothing,
    });
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter') handleSubmit();
  };

  return (
    <Stack gap="sm">
      <TextInput
        label="Phrases"
        placeholder="rust, go, python"
        description="Comma-separated phrases (max 10)"
        value={phrases}
        onChange={e => setPhrases(e.currentTarget.value)}
        onKeyDown={handleKeyDown}
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
        <Select
          label="Granularity"
          data={[
            { value: 'day', label: 'Day' },
            { value: 'week', label: 'Week' },
            { value: 'month', label: 'Month' },
            { value: 'year', label: 'Year' },
          ]}
          value={granularity}
          onChange={v => setGranularity(v || 'month')}
          allowDeselect={false}
        />
      </Group>
      <Group align="end">
        <div style={{ flex: 1 }}>
          <Slider
            label={v => `Smoothing: ${v}`}
            min={0}
            max={12}
            step={1}
            value={smoothing}
            onChange={setSmoothing}
            marks={[
              { value: 0, label: '0' },
              { value: 3, label: '3' },
              { value: 6, label: '6' },
              { value: 12, label: '12' },
            ]}
          />
        </div>
        <Button onClick={handleSubmit}>Search</Button>
      </Group>
    </Stack>
  );
}

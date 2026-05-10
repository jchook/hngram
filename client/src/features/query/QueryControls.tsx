import { useState, useEffect } from 'react';
import {
  Anchor,
  Button,
  Collapse,
  Group,
  Input,
  NumberInput,
  SegmentedControl,
  Stack,
  Text,
  TextInput,
  Tooltip,
} from '@mantine/core';
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
  // Advanced controls are hidden by default — expand if the user has
  // diverged from defaults (e.g. arrived via a shared URL with ?since=2006).
  const [showAdvanced, setShowAdvanced] = useState(
    state.since !== '2011' || state.smoothing !== 3
  );

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

  const startYearLabel = (
    <Group gap={4} wrap="nowrap">
      <span>Start year</span>
      <Tooltip
        label="2011 was the first year Hacker News had over 1M comments. Earlier years are sparse."
        position="top-start"
        withArrow
        multiline
        w={260}
      >
        <Text component="span" c="dimmed" size="xs" style={{ cursor: 'help' }}>
          ⓘ
        </Text>
      </Tooltip>
    </Group>
  );

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

      <Anchor
        component="button"
        type="button"
        size="sm"
        c="dimmed"
        underline="never"
        onClick={() => setShowAdvanced(v => !v)}
        style={{ alignSelf: 'flex-start' }}
      >
        {showAdvanced ? '▴ Hide' : '▾ Show'} advanced options
      </Anchor>

      <Collapse in={showAdvanced}>
        <Group grow align="end">
          <Input.Wrapper label={startYearLabel}>
            <SegmentedControl
              value={since}
              onChange={v => setSince(v as Since)}
              data={SINCE_OPTIONS.map(s => ({ label: `Since ${s}`, value: s }))}
              fullWidth
            />
          </Input.Wrapper>
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
      </Collapse>

      <Group justify="center" pt="md" pb="xs">
        <Button type="submit" size="md" px="xl" style={{ flex: 1, maxWidth: 360 }}>Show Trends</Button>
      </Group>
    </Stack>
    </form>
  );
}

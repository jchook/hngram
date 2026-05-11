import { Fragment, useState, useEffect } from 'react';
import {
  Anchor,
  Button,
  Collapse,
  Group,
  Input,
  NumberInput,
  SegmentedControl,
  Stack,
  TagsInput,
  Text,
  Tooltip,
} from '@mantine/core';
import type { QueryState, Since, YScale } from './useQueryState';
import { SINCE_OPTIONS } from './useQueryState';
import suggested from '../../../../config/suggested-comparisons.json';

const SUGGESTED_COMPARISONS: string[][] = suggested.comparisons;

interface QueryControlsProps {
  state: QueryState;
  onSubmit: (next: Partial<QueryState>) => void;
}

export function QueryControls({ state, onSubmit }: QueryControlsProps) {
  // Local ephemeral state for inputs (committed on submit)
  const [phrases, setPhrases] = useState<string[]>(state.phrases);
  const [since, setSince] = useState<Since>(state.since);
  const [smoothing, setSmoothing] = useState(state.smoothing);
  const [yScale, setYScale] = useState<YScale>(state.yScale);
  // More options are hidden by default — expand if the user has
  // diverged from defaults (e.g. arrived via a shared URL with ?since=2006).
  const [showMore, setShowMore] = useState(
    state.since !== '2011' || state.smoothing !== 3 || state.yScale !== 'linear'
  );

  // Sync from external state changes (e.g. popstate)
  useEffect(() => {
    setPhrases(state.phrases);
    setSince(state.since);
    setSmoothing(state.smoothing);
    setYScale(state.yScale);
  }, [state]);

  const handleSubmit = () => {
    const cleaned = phrases.map(s => s.trim()).filter(Boolean).slice(0, 10);
    if (cleaned.length === 0) return;
    onSubmit({ phrases: cleaned, since, smoothing, yScale });
  };

  const applyComparison = (compPhrases: string[]) => {
    setPhrases(compPhrases);
    onSubmit({ phrases: compPhrases, since, smoothing, yScale });
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
      <TagsInput
        className="phrases-input"
        label="Phrases"
        placeholder={phrases.length === 0 ? 'rust, go, python' : 'Add phrase'}
        description="Press comma to add (max 10)"
        value={phrases}
        onChange={setPhrases}
        splitChars={[',']}
        maxTags={10}
        clearable
      />

      <Anchor
        component="button"
        type="button"
        size="sm"
        c="dimmed"
        underline="never"
        onClick={() => setShowMore(v => !v)}
        style={{ alignSelf: 'flex-start' }}
      >
        {showMore ? '▾ Show less' : '▸ Show more'}
      </Anchor>

      <Collapse in={showMore}>
        <Stack gap="sm" pb="sm">
          <Input.Wrapper
            label="Try a comparison"
            description="Click to load a curated phrase set"
          >
            <Group gap="xs" mt={4}>
              {SUGGESTED_COMPARISONS.map((compPhrases, i) => (
                <Button
                  key={i}
                  type="button"
                  variant="light"
                  size="xs"
                  onClick={() => applyComparison(compPhrases)}
                >
                  {compPhrases.map((phrase, j) => (
                    <Fragment key={j}>
                      {j > 0 && (
                        <span style={{ opacity: 0.4, padding: '0 0.4em' }}>•</span>
                      )}
                      {phrase}
                    </Fragment>
                  ))}
                </Button>
              ))}
            </Group>
          </Input.Wrapper>

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

          <Input.Wrapper
            label="Y axis"
            description="Use log scale to compare phrases with very different popularity"
          >
            <SegmentedControl
              value={yScale}
              onChange={v => setYScale(v === 'log' ? 'log' : 'linear')}
              data={[
                { label: 'Linear', value: 'linear' },
                { label: 'Log', value: 'log' },
              ]}
              mt={4}
            />
          </Input.Wrapper>
        </Stack>
      </Collapse>

      <Group justify="center" pb="xs">
        <Button
          type="submit"
          size="md"
          px="xl"
          variant="outline"
          style={{ flex: 1, maxWidth: 360, borderWidth: 2 }}
        >
          Show Trends
        </Button>
      </Group>
    </Stack>
    </form>
  );
}

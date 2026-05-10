import { useEffect, useMemo, useState } from 'react';
import { Anchor, Text, Transition } from '@mantine/core';
import { useFeedback } from '@/gen';

interface Props {
  phrases: string[];
  start: string;
  granularity: string;
  smoothing: number;
}

function storageKey(phrases: string[], start: string, granularity: string): string {
  const sorted = [...phrases].sort().join('|');
  return `hngram:feedback:${sorted}::${start}::${granularity}`;
}

function readRated(key: string): boolean {
  try {
    return window.localStorage.getItem(key) === 'rated';
  } catch {
    return false;
  }
}

function writeRated(key: string): void {
  try {
    window.localStorage.setItem(key, 'rated');
  } catch {
    // ignore quota / private-mode errors
  }
}

export function FeedbackBanner({ phrases, start, granularity, smoothing }: Props) {
  const key = useMemo(
    () => storageKey(phrases, start, granularity),
    [phrases, start, granularity],
  );

  const [alreadyRated, setAlreadyRated] = useState(() => readRated(key));
  const [submitted, setSubmitted] = useState(false);

  useEffect(() => {
    setAlreadyRated(readRated(key));
    setSubmitted(false);
  }, [key]);

  const { mutate, isPending } = useFeedback({
    mutation: {
      onSuccess: () => {
        writeRated(key);
        setSubmitted(true);
      },
    },
  });

  if (alreadyRated) return null;

  const handleYes = () => {
    if (isPending) return;
    mutate({ data: { phrases, start, granularity, smoothing } });
  };

  return (
    <div style={{ position: 'relative', minHeight: 20, textAlign: 'center' }}>
      <Transition mounted={!submitted} transition="fade" duration={200}>
        {styles => (
          <Text
            size="sm"
            c="dimmed"
            style={{ ...styles, position: 'absolute', inset: 0 }}
          >
            Is this trend comparison particularly interesting?{' '}
            <Anchor
              component="button"
              type="button"
              c="orange"
              fw={500}
              underline="hover"
              onClick={handleYes}
              disabled={isPending}
            >
              Yes
            </Anchor>
          </Text>
        )}
      </Transition>
      <Transition mounted={submitted} transition="fade" duration={400}>
        {styles => (
          <Text
            size="sm"
            c="dimmed"
            style={{ ...styles, position: 'absolute', inset: 0 }}
          >
            Thanks for your feedback!
          </Text>
        )}
      </Transition>
    </div>
  );
}

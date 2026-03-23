import { Alert, Group, Loader, Text } from '@mantine/core';
import type { QueryResponse } from '@/gen';

interface QueryResult {
  isLoading: boolean;
  error: { message?: string } | null;
  data?: QueryResponse;
}

interface QueryStatusProps {
  phrases: string[];
  results: QueryResult[];
}

export function QueryStatus({ phrases, results }: QueryStatusProps) {
  const items: React.ReactNode[] = [];

  for (let i = 0; i < phrases.length; i++) {
    const result = results[i];
    if (!result) continue;

    if (result.isLoading) {
      items.push(
        <Group key={phrases[i]} gap="xs">
          <Loader size="xs" />
          <Text size="sm" c="dimmed">Loading "{phrases[i]}"...</Text>
        </Group>
      );
    } else if (result.error) {
      items.push(
        <Alert key={phrases[i]} color="red" variant="light" py="xs">
          Error querying "{phrases[i]}": {result.error.message || 'Unknown error'}
        </Alert>
      );
    } else if (result.data?.status === 'not_indexed') {
      items.push(
        <Alert key={phrases[i]} color="yellow" variant="light" py="xs">
          "{phrases[i]}" is not indexed (too rare historically)
        </Alert>
      );
    } else if (result.data?.status === 'invalid') {
      items.push(
        <Alert key={phrases[i]} color="red" variant="light" py="xs">
          "{phrases[i]}" is invalid (must be 1-3 words)
        </Alert>
      );
    }
  }

  if (items.length === 0) return null;

  return <>{items}</>;
}

import { Alert, Group, Loader, Text } from '@mantine/core';
import type { QueryResponse } from '@/gen';
import { ApiError } from '@/lib/client';

interface QueryResult {
  isLoading: boolean;
  isFetching?: boolean;
  failureCount?: number;
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

    const failureCount = result.failureCount ?? 0;
    const isRetrying = failureCount > 0 && (result.isFetching ?? false);

    if (result.isLoading || isRetrying) {
      const label = isRetrying
        ? `Retrying "${phrases[i]}" (server is busy, attempt ${failureCount + 1})...`
        : `Loading "${phrases[i]}"...`;
      items.push(
        <Group key={phrases[i]} gap="xs">
          <Loader size="xs" />
          <Text size="sm" c="dimmed">{label}</Text>
        </Group>
      );
    } else if (result.error) {
      const isServerError =
        result.error instanceof ApiError && result.error.status >= 500;
      const message = isServerError
        ? `"${phrases[i]}": the chart service is busy right now — please retry in a moment.`
        : `Error querying "${phrases[i]}": ${result.error.message || 'Unknown error'}`;
      items.push(
        <Alert key={phrases[i]} color={isServerError ? 'yellow' : 'red'} variant="light" py="xs">
          {message}
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
          "{phrases[i]}" is invalid (must be 1-5 words)
        </Alert>
      );
    }
  }

  if (items.length === 0) return null;

  return <>{items}</>;
}

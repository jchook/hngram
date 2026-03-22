import { Container, Title, Text, TextInput, Button, Group, Stack, Paper } from '@mantine/core';
import { useState } from 'react';

export default function App() {
  const [phrases, setPhrases] = useState('rust, go, python');

  return (
    <Container size="lg" py="xl">
      <Stack gap="lg">
        <div>
          <Title order={1}>HN N-gram Viewer</Title>
          <Text c="dimmed">
            Explore word and phrase trends in Hacker News comments over time
          </Text>
        </div>

        <Paper shadow="xs" p="md" withBorder>
          <Stack gap="md">
            <TextInput
              label="Phrases"
              description="Enter comma-separated phrases to compare"
              placeholder="rust, go, python"
              value={phrases}
              onChange={(e) => setPhrases(e.target.value)}
            />

            <Group>
              <Button>Search</Button>
            </Group>
          </Stack>
        </Paper>

        <Paper shadow="xs" p="md" withBorder>
          <Text c="dimmed" ta="center" py="xl">
            Chart will appear here once the API is connected
          </Text>
        </Paper>
      </Stack>
    </Container>
  );
}

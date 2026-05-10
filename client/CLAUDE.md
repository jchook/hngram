# HN N-gram Client

React frontend for the HN N-gram Viewer. See the root `CLAUDE.md` for repo-wide context (OpenAPI → SDK pipeline, env vars, deployment).

## Stack

- **React 18** + **TypeScript** (strict)
- **Mantine 7** — UI components, theming, dates (`@mantine/core`, `@mantine/hooks`, `@mantine/dates`). `@mantine/dates` must stay co-versioned with `@mantine/core`.
- **TanStack Query v5** — server-state cache; one query per phrase (see root CLAUDE.md gotcha).
- **Kubb** — generates the typed SDK + React Query hooks from `server/openapi.json` into `src/gen/` (gitignored).
- **ECharts** via `echarts-for-react` — chart rendering.
- **Rsbuild** — dev server and bundler.
- **Biome** — lint + format. **Bun** — runtime / package manager.

## Coding practices

### No `as` type assertions

Do not write `value as SomeType` (or `as unknown as T`). They silently disable the type checker — at best they hide a real type bug, at worst they let bad data flow through the app and crash at runtime.

Instead:
- Annotate the variable: `const x: SomeType = …` and let inference / the constructor enforce it.
- Fix the upstream type so the assertion isn't needed (often the right answer when fighting an SDK or library type).
- Use a type guard (`if (isFoo(x)) …`) or a runtime parser when crossing a real boundary.
- `as const` for literal narrowing is fine — it's a widening prevention, not an assertion.

If you genuinely need an escape hatch, stop and ask the user first.

### Theme-based styling, not one-offs

Mantine theme lives in `src/main.tsx` (`createTheme({...})`). Global CSS variables and HN-flavored overrides are in `src/index.css`. Phrase-pill colors are derived at startup from `features/chart/colors.ts` so the chart palette is the single source of truth.

When adding or restyling UI:
- Use Mantine component props (`size`, `c`, `bg`, `radius`, spacing tokens like `"sm"` / `"md"`) — not inline `style={{ … }}` with raw pixels.
- For repeated patterns, extend the theme (component defaults, custom colors, spacing) rather than sprinkling literals.
- If a one-off truly is one-off, prefer a CSS class in `index.css` (or a CSS module) over an inline style — it stays grep-able and themable.
- Pull colors from theme / `colors.ts`, never from hardcoded hex in components.

If you find yourself reaching for an inline style or a magic number, that's usually a signal the theme should grow instead.

### Hooks: assume infinite-loop risk by default

`useEffect`, `useMemo`, `useCallback`, and TanStack Query's `queryKey` all do referential-equality dependency checks. The common traps:

- New object/array literal in deps: `useEffect(() => {...}, [{ foo }])` — re-runs every render.
- Function recreated each render passed as a dep — wrap in `useCallback` (with correct deps) or move it out.
- Setting state inside `useEffect` without a guard that eventually stops the cascade.
- TanStack `queryKey` containing a fresh object each render — turns caching off and re-fetches forever.

Before adding/editing a hook: write down what triggers a re-run, then confirm each dep is stable across renders that shouldn't trigger one. When in doubt, log inside the effect during dev to confirm it fires the expected number of times.

### All API calls go through the generated SDK

Never hand-write `fetch(...)` against the API. Use the hooks/functions in `src/gen/` (e.g. `useNgram`, `getNgram`). They are typed end-to-end from the Rust handler signatures.

If the API shape changes (new endpoint, changed request/response type), the SDK is **stale until regenerated**. The pipeline:

```bash
cd server && just openapi    # writes server/openapi.json from Rust types
cd client && just gen        # writes client/src/gen/ from openapi.json
```

Both steps are required — `just openapi` alone won't update the client; `just gen` alone will regenerate from a stale spec. After regen, run `bun run typecheck` to surface any callsites that need updating.

See the root `CLAUDE.md` "Adding a new endpoint" section for the four API-side registration points (handler, `ApiDoc` paths, `ApiDoc` schemas, router) — missing any one means the change won't reach the SDK.

## Where to look

| What | Where |
|------|-------|
| App entry / theme / QueryClient | `src/main.tsx` |
| Top-level layout | `src/App.tsx` |
| URL-state hook | `src/features/query/useQueryState.ts` |
| Chart palette + transforms | `src/features/chart/` |
| Generated SDK (read-only) | `src/gen/` |
| Kubb config | `kubb.config.ts` |
| SDK adapter contract | `src/lib/client.ts` |

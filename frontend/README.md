# Sigil Search — Web UI

A Splunk-style search workflow in an Elastic/Kibana-style UI (built with the
official **EUI** component library). This is the "Kibana analog" for the Sigil
stack — it talks to the existing query API (`/search`, `/query`, `/sql`).

![Discover + Statistics](../docs) <!-- screenshots optional -->

## What it does

- **One search bar, three languages** — toggle **Pipe-DSL** (Splunk SPL-like,
  the default), **Lucene** query-string, or raw **SQL** (DataFusion). The lowered
  SQL is shown for DSL queries.
- **Time picker** (`EuiSuperDatePicker`) — quick/relative/absolute ranges.
- **Events (Discover)** — an events-over-time histogram, an expandable events
  table (full fields + raw JSON), and a left **field sidebar** with top values
  you can click to add filters.
- **Statistics** — when a query aggregates (`... | stats count by host` or a SQL
  `GROUP BY`) it shows a results table + bar chart.
- **Dashboards** — build dashboards of panels, each a saved query rendered as a
  **metric**, **bar chart**, **table**, or **time series**. "Add to dashboard"
  from any search; dashboards persist in localStorage.
- **Light/dark theme** and **recent-query history** (localStorage).

## Run (dev)

```bash
# 1. start the backend (in the repo root)
cargo run -p sigil-cli -- run

# 2. seed a little data
printf '%s\n' '{"index":{"_index":"web"}}' '{"message":"Failed password for root","host":{"name":"web1"},"log":{"level":"error"}}' \
  | curl -s :9200/_bulk -H 'Content-Type: application/x-ndjson' --data-binary @-

# 3. start the UI
cd frontend && npm install && npm run dev      # http://localhost:5173
```

In dev, Vite proxies `/api/*` to `http://127.0.0.1:9595` (no CORS needed). Set
`VITE_API_TARGET` to point at a different backend.

## Run (Docker, full stack)

```bash
docker compose -f deploy/docker-compose.yml up --build
#  UI  -> http://localhost:8080   (nginx serves the SPA + proxies /api -> backend)
#  API -> http://localhost:9595
```

## Notes / limitations

- **Time filtering** is server-side for SQL (`WHERE ts …`) and client-side for
  the returned events in DSL/Lucene modes (the DSL `where` grammar has no range
  operators; `/search` has no time param yet). The histogram is built from the
  returned events.
- Event `ts` is currently **ingest time** (timestamp parsing is a backend TODO),
  so seeded events cluster near "now" in the histogram.
- Charts are lightweight inline SVG (no chart dependency) themed with EUI colors;
  they can be swapped for `@elastic/charts` later.

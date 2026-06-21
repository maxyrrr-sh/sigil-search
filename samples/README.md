# Sample dataset

A realistic, varied log dataset for trying out Sigil Search (search, SQL, the
pipe-DSL, detections, and the web UI).

- [`events.ndjson`](events.ndjson) — ~425 events, one JSON object per line, across
  three datasets:
  - **web** — nginx-style access logs (method, path, status, bytes, client IP)
  - **app** — service logs (api/worker/scheduler/payments) at info/debug/warn/error
  - **auth** — sshd/sudo logs, **including a brute-force burst** from `203.0.113.66`
    so detection + correlation demos have signal
- [`generate.py`](generate.py) — regenerate it: `python3 samples/generate.py 800 > samples/events.ndjson`
- [`seed.sh`](seed.sh) — load it into a running backend via ES `_bulk`

> Synthetic but modeled on common real shapes. For real open datasets see
> [LogHub](https://github.com/logpai/loghub) or Elastic's sample data; convert
> them to JSON lines and load the same way.

## Load it

**Into a running server** (recommended — uses the ES `_bulk` endpoint, preserves
per-dataset routing):

```bash
sigil-search run ./configs/sigil-search.yaml &     # backend up (API :9595, _bulk :9200)
./samples/seed.sh                                  # -> http://localhost:9200
```

**Offline** (server stopped) via replay (everything lands under dataset `replay`):

```bash
sigil-search replay samples/events.ndjson ./configs/sigil-search.yaml
```

## Try it

```bash
# top status codes
curl -G localhost:9595/sql --data-urlencode \
  "q=SELECT host, count(*) c FROM events GROUP BY host ORDER BY c DESC"

# the brute-force source, Splunk-style
curl -G localhost:9595/query --data-urlencode \
  "q=search Failed password | stats count by host"

# full-text
curl 'localhost:9595/search?q=message:checkout%20AND%20log.level:error'
```

Or open the web UI (`frontend/`) and explore visually.

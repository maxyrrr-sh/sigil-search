#!/usr/bin/env bash
# Load the sample dataset into a RUNNING Sigil backend via the ES `_bulk` endpoint.
#
#   ./samples/seed.sh                      # -> http://localhost:9200
#   ./samples/seed.sh http://host:9200
#
# Each event's `_dataset` becomes the ES `_index` (web | app | auth).
set -euo pipefail

ENDPOINT="${1:-http://localhost:9200}"
DIR="$(cd "$(dirname "$0")" && pwd)"
FILE="$DIR/events.ndjson"

[ -f "$FILE" ] || { echo "missing $FILE — run: python3 samples/generate.py > samples/events.ndjson"; exit 1; }

echo "Seeding $(wc -l < "$FILE" | tr -d ' ') events to $ENDPOINT/_bulk ..."

# Expand each event into an action line + the document (dropping the _dataset hint).
python3 - "$FILE" <<'PY' | curl -s -XPOST "$ENDPOINT/_bulk" -H 'Content-Type: application/x-ndjson' --data-binary @- \
  | python3 -c 'import sys,json; d=json.load(sys.stdin); print("done — errors:", d.get("errors"), "items:", len(d.get("items",[])))'
import json, sys
for line in open(sys.argv[1]):
    line = line.strip()
    if not line:
        continue
    doc = json.loads(line)
    index = doc.pop("_dataset", "sample")
    print(json.dumps({"index": {"_index": index}}))
    print(json.dumps(doc))
PY

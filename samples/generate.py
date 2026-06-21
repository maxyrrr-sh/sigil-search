#!/usr/bin/env python3
"""Generate a realistic, varied sample log dataset for Sigil Search.

Synthetic (so it ships with the repo and matches the ECS-ish schema), but modeled
on common real shapes: nginx access logs, app service logs, and sshd/sudo auth
logs — including a brute-force burst so detection/correlation demos have signal.

Each line is one self-contained JSON event with nested fields plus a top-level
`_dataset` hint (web | app | auth) used by seed.sh to pick the ES `_index`.

Usage:  python3 samples/generate.py [count] > samples/events.ndjson
"""
import json
import random
import sys
from datetime import datetime, timedelta, timezone

random.seed(1729)

HOSTS_WEB = ["web1", "web2", "web3"]
HOSTS_APP = ["app1", "app2"]
HOSTS_AUTH = ["bastion", "web1"]

PRIVATE_IPS = [f"10.0.0.{i}" for i in range(2, 40)]
PUBLIC_IPS = ["203.0.113.7", "198.51.100.23", "8.8.8.8", "192.0.2.44", "203.0.113.9"]
BRUTE_IP = "203.0.113.66"  # repeated attacker for the brute-force burst

PATHS = ["/", "/login", "/api/users", "/api/orders", "/health", "/static/app.js",
         "/checkout", "/search", "/admin", "/api/products"]
METHODS = (["GET"] * 6) + (["POST"] * 3) + ["PUT", "DELETE"]
STATUSES = ([200] * 10) + ([201, 304] * 2) + [301, 400, 401, 403, 404, 500, 503]
USERS = ["alice", "bob", "carol", "root", "admin", "deploy", "svc-ci"]

APP_SERVICES = ["api", "worker", "scheduler", "payments"]
APP_TEMPLATES = [
    ("info", "request completed in {ms}ms status={st}"),
    ("info", "user {u} updated profile"),
    ("debug", "cache miss for key user:{n}"),
    ("debug", "heartbeat tick {n}"),
    ("warning", "rate limit exceeded for {ip}"),
    ("warning", "slow query took {ms}ms"),
    ("error", "db connection timeout to primary"),
    ("error", "job {n} failed: upstream 503"),
]

now = datetime.now(timezone.utc)


def iso(dt):
    return dt.isoformat().replace("+00:00", "Z")


def web_event(dt):
    host = random.choice(HOSTS_WEB)
    method = random.choice(METHODS)
    path = random.choice(PATHS)
    status = random.choice(STATUSES)
    ip = random.choice(PRIVATE_IPS + PUBLIC_IPS)
    size = random.randint(120, 48000)
    level = "info" if status < 400 else ("warning" if status < 500 else "error")
    return {
        "_dataset": "web",
        "@timestamp": iso(dt),
        "message": f'{ip} - - "{method} {path} HTTP/1.1" {status} {size}',
        "host": {"name": host},
        "service": {"name": "nginx"},
        "source": {"ip": ip},
        "http": {"request": {"method": method},
                 "response": {"status_code": status, "bytes": size}},
        "url": {"path": path},
        "log": {"level": level},
    }


def app_event(dt):
    host = random.choice(HOSTS_APP)
    service = random.choice(APP_SERVICES)
    level, tmpl = random.choice(APP_TEMPLATES)
    msg = tmpl.format(ms=random.randint(3, 2400), st=random.choice([200, 500]),
                      u=random.choice(USERS), n=random.randint(1, 9999),
                      ip=random.choice(PUBLIC_IPS))
    return {
        "_dataset": "app",
        "@timestamp": iso(dt),
        "message": msg,
        "host": {"name": host},
        "service": {"name": service},
        "log": {"level": level},
    }


def auth_event(dt, brute=False):
    host = random.choice(HOSTS_AUTH)
    if brute:
        user = random.choice(["root", "admin", "oracle", "test"])
        ip = BRUTE_IP
        port = random.randint(30000, 60000)
        return {
            "_dataset": "auth",
            "@timestamp": iso(dt),
            "message": f"Failed password for {user} from {ip} port {port} ssh2",
            "host": {"name": host},
            "service": {"name": "sshd"},
            "source": {"ip": ip},
            "user": {"name": user},
            "event": {"action": "logon-failed", "outcome": "failure"},
            "log": {"level": "warning"},
        }
    roll = random.random()
    if roll < 0.5:
        user = random.choice(USERS)
        ip = random.choice(PRIVATE_IPS)
        return {
            "_dataset": "auth", "@timestamp": iso(dt),
            "message": f"Accepted password for {user} from {ip} port {random.randint(30000,60000)} ssh2",
            "host": {"name": host}, "service": {"name": "sshd"}, "source": {"ip": ip},
            "user": {"name": user}, "event": {"action": "logon-success", "outcome": "success"},
            "log": {"level": "info"},
        }
    elif roll < 0.8:
        user = random.choice(["postgres", "nobody", "x", "ubuntu"])
        ip = random.choice(PUBLIC_IPS)
        return {
            "_dataset": "auth", "@timestamp": iso(dt),
            "message": f"Invalid user {user} from {ip} port {random.randint(30000,60000)}",
            "host": {"name": host}, "service": {"name": "sshd"}, "source": {"ip": ip},
            "user": {"name": user}, "event": {"action": "logon-failed", "outcome": "failure"},
            "log": {"level": "warning"},
        }
    else:
        user = random.choice(["alice", "deploy"])
        return {
            "_dataset": "auth", "@timestamp": iso(dt),
            "message": f"session opened for user root by {user}(uid=0)",
            "host": {"name": host}, "service": {"name": "sudo"},
            "user": {"name": user}, "event": {"action": "session-opened"},
            "log": {"level": "info"},
        }


def main():
    count = int(sys.argv[1]) if len(sys.argv) > 1 else 400
    events = []
    # Spread synthetic @timestamps over the last 6 hours (note: the backend
    # currently indexes by ingest time; @timestamp is kept as a field).
    for i in range(count):
        dt = now - timedelta(seconds=random.randint(0, 6 * 3600))
        r = random.random()
        if r < 0.55:
            events.append(web_event(dt))
        elif r < 0.85:
            events.append(app_event(dt))
        else:
            events.append(auth_event(dt))
    # A concentrated brute-force burst from one IP (for detection/correlation).
    burst_start = now - timedelta(minutes=4)
    for j in range(25):
        events.append(auth_event(burst_start + timedelta(seconds=j * 3), brute=True))

    events.sort(key=lambda e: e["@timestamp"])
    for e in events:
        sys.stdout.write(json.dumps(e) + "\n")


if __name__ == "__main__":
    main()

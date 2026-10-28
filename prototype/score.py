"""Turn labeled.jsonl (from detect.py) into the numbers on the scorecard.

Usage: python3 score.py labeled.jsonl > scorecard.json
"""
import json, sys
from collections import Counter, defaultdict
from datetime import datetime, timedelta

CATCH = {"confirmed", "admitted", "claimed"}
TIERS = [(15, "Irreplaceable"), (7, "Sharp-eyed"), (3, "Coworker"), (0, "Rubber stamp")]
REAL_CHAT = 10  # a session with at least this many human messages counts as a "real chat"

rows = [json.loads(l) for l in open(sys.argv[1])]
rows = [r for r in rows if r["ts"]]
for r in rows:
    r["d"] = datetime.fromisoformat(r["ts"].replace("Z", "+00:00"))
catches = [r for r in rows if r["label"] in CATCH]
now = max(r["d"] for r in rows)
week = [r for r in catches if r["d"] > now - timedelta(days=7)]

sessions = defaultdict(lambda: [0, 0])
for r in rows:
    sessions[r["session"]][0] += 1
    sessions[r["session"]][1] += r["label"] in CATCH
real = [s for s in sessions.values() if s[0] >= REAL_CHAT]

days = sorted({r["d"].date() for r in catches})
best = cur = 1 if days else 0
for a, b in zip(days, days[1:]):
    cur = cur + 1 if (b - a).days == 1 else 1
    best = max(best, cur)

weekly = Counter((r["d"] - timedelta(days=r["d"].weekday())).date().isoformat() for r in catches)
hours = Counter(r["d"].astimezone().hour for r in catches)

print(json.dumps({
    "range": [min(r["d"] for r in rows).date().isoformat(), now.date().isoformat()],
    "messages": len(rows),
    "catches": len(catches),
    "humbled": sum(r["label"] == "humbled" for r in rows),
    "labels": Counter(r["label"] for r in rows if r["label"]),
    "catches_last_7d": len(week),
    "tier": next(name for floor, name in TIERS if len(week) >= floor),
    "messages_per_catch": round(len(rows) / max(1, len(catches))),
    "real_chats": len(real),
    "pct_real_chats_with_catch": round(100 * sum(1 for s in real if s[1]) / max(1, len(real)), 1),
    "best_streak_days": best,
    "peak_hour": hours.most_common(1)[0][0] if hours else None,
    "categories": Counter(r["category"] for r in catches),
    "weekly": dict(sorted(weekly.items())),
}, indent=2))

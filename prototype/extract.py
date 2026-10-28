"""Extract human turns from Claude Code transcripts with surrounding AI text.

Output: turns.jsonl, one row per human message:
  {src, session, project, ts, text, prev_ai, next_ai}
"""
import glob, json, os, re, sys

ROOT = os.path.expanduser("~/.claude/projects")
OUT = sys.argv[1]
SKIP_SESSION = os.environ.get("WA_SKIP_SESSION", "")  # optional: exclude the current session

NOISE = re.compile(
    r"^\s*(<command-|<local-command|<system-reminder|<task-notification|<bash-|"
    r"\[Request interrupted|Caveat:|This session is being continued|<user-prompt-submit-hook|"
    r"<side-sessions-event|<ci-monitor-event|\[Artifact comment)"
)


def text_of(content):
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        if any(isinstance(x, dict) and x.get("type") == "tool_result" for x in content):
            return None
        parts = [x.get("text", "") for x in content if isinstance(x, dict) and x.get("type") == "text"]
        return "\n".join(parts) if parts else None
    return None


seen = set()
n = 0
with open(OUT, "w") as out:
    for f in glob.glob(f"{ROOT}/*/*.jsonl"):
        session = os.path.basename(f)[:-6]
        if session == SKIP_SESSION:
            continue
        project = os.path.basename(os.path.dirname(f))
        project = re.sub(r"^-Users-[^-]+-(Web-)?", "", project)
        if "scratch-workspaces" in project:
            project = "scratch"
        events = []  # (kind, text, ts)
        try:
            fh = open(f, errors="ignore")
        except OSError:
            continue
        for line in fh:
            try:
                d = json.loads(line)
            except ValueError:
                continue
            if d.get("isSidechain") or d.get("isMeta"):
                continue
            t = d.get("type")
            ts = d.get("timestamp", "")
            if t == "user":
                txt = text_of((d.get("message") or {}).get("content"))
                if txt and not NOISE.match(txt):
                    events.append(("human", txt, ts))
            elif t == "attachment":
                a = d.get("attachment") or {}
                if a.get("type") == "queued_command" and isinstance(a.get("prompt"), str):
                    if not NOISE.match(a["prompt"]):
                        events.append(("human", a["prompt"], ts))
            elif t == "assistant":
                txt = text_of((d.get("message") or {}).get("content"))
                if txt:
                    events.append(("ai", txt, ts))
        for i, (k, txt, ts) in enumerate(events):
            if k != "human":
                continue
            key = (txt.strip()[:300], ts[:16])
            if key in seen:
                continue
            seen.add(key)
            prev_ai = next((e[1] for e in reversed(events[:i]) if e[0] == "ai"), None)
            nxt = None
            for e in events[i + 1:]:
                if e[0] == "human":
                    break
                if e[0] == "ai":
                    nxt = e[1]
                    break
            out.write(json.dumps({
                "src": "claude-code", "session": session, "project": project, "ts": ts,
                "text": txt[:4000], "prev_ai": (prev_ai or "")[-1500:], "next_ai": (nxt or "")[:1200],
            }) + "\n")
            n += 1
print("human turns:", n)

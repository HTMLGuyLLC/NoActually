"""Deterministic correction detector.

Two independent signals per human turn:
  1. USER signal  - the human's message uses correction language.
  2. AI signal    - the AI's very next reply admits the mistake.
Both  -> "confirmed" catch (highest confidence)
AI    -> "admitted" catch
User  -> "claimed" catch (only counted when it opens the message)
Separately, "humbled" = the AI's reply pushes back on the human.
"""
import json, re, sys

def rx(*pats):
    return re.compile("|".join(pats), re.I)

# AI admits it was wrong (checked in the first 400 chars of the next reply)
AI_ADMIT = rx(
    r"\byou'?re (absolutely |totally |completely )?right\b", r"\byou are (absolutely )?right\b",
    r"\b(good|great|nice|sharp) catch\b", r"\bmy (mistake|bad|apologies|error)\b",
    r"\bi (was wrong|got (that|it) wrong|missed|misread|overlooked|misunderstood|made (a|an) (mistake|error))\b",
    r"\bi apologi[sz]e\b", r"\bsorry,? (about that|for (the|that))\b", r"\bthat'?s on me\b",
    r"\bi should have\b", r"\bi incorrectly\b", r"\bthat was (wrong|incorrect|a mistake)\b",
    r"\bi (didn'?t|did not) (actually )?(check|verify|read|catch)\b",
)

# User correction language, matched at the start of the message
USER_OPEN = rx(
    r"^(no|nope|nah)\s*[,.!—–-]+\s*(that|this|it|you|not|the|we|i said|still|wrong)\b", r"^(no|nope|nah|wrong)[.!]*$",
    r"^(wrong|incorrect|not quite|not true|that'?s not|that is not|that isn'?t|this is not|this isn'?t)\b",
    r"^(it'?s|that'?s|this is) (still )?(wrong|incorrect|broken|not (right|correct|working))",
    r"^still (not|seeing|broken|getting|showing|can'?t|cannot|no|wrong|doesn'?t|isn'?t|the same|failing|happening|there)\b", r"^(you|u) (missed|forgot|didn'?t|did not|ignored|broke|skipped|never)\b",
    r"^(i said|i told you|like i said|as i said|i already (said|told|asked))\b", r"^i asked (you )?(to|for|not)\b",
    r"^(why did you|why would you|what\??$|wtf|huh\??$)",
    r"^(doesn'?t|didn'?t|does not|did not|isn'?t|not) work",
    r"^(same|still the same) (error|issue|problem|thing)",
    r"^actually,? (it|that|this|you|the|no)\b",
)
# Correction language anywhere (only counts when the AI also admits it)
USER_ANY = rx(
    r"\b(doesn'?t|does not|didn'?t|did not) (exist|work)\b", r"\bnot working\b", r"\bstill (broken|failing|not|wrong|the same|showing|getting)\b",
    r"\byou (missed|forgot|ignored|broke|skipped|didn'?t (read|check|test|follow|do))\b",
    r"\b(that'?s|this is|it'?s) (wrong|incorrect|not right)\b", r"\bmade (that|it) up\b", r"\bhallucinat",
    r"\bi (said|told you|asked( you)? (to|for|not))\b", r"\bnot what i (asked|wanted|said|meant)\b",
)
# The human added info the AI couldn't have known: neutral, never a catch
CONTEXT = rx(r"^(also|oh and|btw|fyi|one more thing|additionally|and also|plus)\b",
             r"\bi forgot to (mention|say|tell)\b", r"^(ok|okay|great|thanks|perfect|cool|nice|yes|yep|sure)\b")

# The AI pushes back on the human (first 250 chars of the reply)
AI_PUSHBACK = rx(
    r"^\W*(actually|not quite|small correction|one correction|quick correction|to clarify)\b",
    r"\b(that'?s|this is|that is) not (quite )?(right|correct|accurate|true|how)\b",
    r"\bi (don'?t|do not) think (that'?s|that is|this is) (right|correct|accurate)\b",
    r"\b(isn'?t|is not) (actually )?(the case|accurate|correct)\b", r"\bi'?d push back\b",
    r"^\W*(it|that) (already|actually) (exists|works|does|is)\b", r"\bthat'?s not a bug\b",
)

CATEGORIES = [  # first match wins
    ("hallucination", rx(r"\b(doesn'?t|does not) exist\b", r"\bmade (that|it) up\b", r"\bhallucinat", r"\bno such\b", r"\bnot a real\b", r"\bthere is no\b", r"\bthat'?s not a thing\b")),
    ("ignored_instructions", rx(r"\bi (said|told you|asked|already)\b", r"\blike i said\b", r"\bnot what i\b", r"\byou ignored\b", r"\bre-?read\b", r"\bi meant\b")),
    ("misread", rx(r"\bi (misread|misunderstood|misinterpreted)\b", r"\bi (read|took) .{0,40} as\b", r"\bnot what i (meant|wanted)\b")),
    ("missed_something", rx(r"\byou (missed|forgot|skipped|didn'?t|did not|never)\b", r"\bmissing\b", r"\bwhat about\b", r"\bleft out\b")),
    ("still_broken", rx(r"\bstill\b", r"\bsame (error|issue|problem)\b", r"\b(doesn'?t|didn'?t|not) work", r"\bbroken\b", r"\berror\b", r"\bcrash", r"\bfail")),
    ("just_wrong", rx(r".")),
]


def classify(row):
    t = row["text"].strip()
    head = t[:300]
    nxt = row["next_ai"][:250]
    ai_admit = bool(AI_ADMIT.search(nxt))
    user_open = bool(USER_OPEN.search(head))
    user_any = bool(USER_ANY.search(t[:1500]))
    context = bool(CONTEXT.search(head))
    if t.startswith("<"):  # tool/agent payloads, not typed by a human
        return {"label": None, "category": None}
    answering = "?" in row["prev_ai"][-300:]  # human is replying to a question
    status_q = len(t) < 60 and re.search(r"\b(finish|done|left|still going|status|ready)\b", t, re.I)
    if len(t) > 2500:  # pasted docs/specs, not conversational corrections
        user_open = user_any = False

    label = None
    if ai_admit and (user_open or user_any):
        label = "confirmed"
    elif ai_admit and not context:
        label = "admitted"
    elif user_open and not context and not answering:
        label = "claimed"
    if label:
        both = t[:1500] + "\n" + nxt  # the AI's admission often names the mistake
        cat = next(name for name, r in CATEGORIES if r.search(both))
        return {"label": label, "category": cat}
    if not status_q and AI_PUSHBACK.search(row["next_ai"][:250]):
        return {"label": "humbled", "category": None}
    return {"label": None, "category": None}


if __name__ == "__main__":
    rows = [json.loads(l) for l in open(sys.argv[1])]
    with open(sys.argv[2], "w") as out:
        for r in rows:
            r.update(classify(r))
            out.write(json.dumps(r) + "\n")

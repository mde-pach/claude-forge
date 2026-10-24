"""Fixed infrastructure-failure classifier (DECISIONS.md D11). Infrastructure is never a result."""
import re


class Infra(Exception):
    pass


class Parity(Exception):
    pass


INFRA_PATTERNS = [
    r"overloaded", r"rate.?limit", r"usage limit", r"limit reached", r"\b5\d\d\b.*(error|status)", r"api error",
    r"ECONNRESET", r"ETIMEDOUT", r"ENOTFOUND", r"EAI_AGAIN", r"socket hang up", r"network error",
    r"fetch failed", r"Connection error", r"not logged in", r"please run /login", r"authentication",
    r"credit balance", r"No space left on device", r"OAuth token", r"Request timed out",
]
INFRA_RX = re.compile("|".join(INFRA_PATTERNS), re.I)


def agent_infra(tinfo, timed_out, stderr):
    """Return a reason string when the agent run failed for infrastructure reasons, else None.
    A timeout is a scored outcome (D10). A run with no `result` event and no timeout is infra.
    A `result` that is an error whose text matches the infra patterns is infra; a budget or
    turn limit is scored."""
    if timed_out:
        return None
    if not tinfo.get("has_result"):
        m = INFRA_RX.search(stderr or "") or INFRA_RX.search(tinfo.get("last_text") or "")
        return "no result event" + (f" ({m.group(0)})" if m else "")
    if tinfo.get("is_error"):
        sub = tinfo.get("result_subtype") or ""
        if sub in ("error_max_budget_usd", "error_max_turns"):
            return None
        text = (tinfo.get("result_text") or "") + " " + (tinfo.get("api_error_text") or "")
        if INFRA_RX.search(text) or sub == "error_during_execution":
            return f"agent error result ({sub}): {text.strip()[:300]}"
    if tinfo.get("api_errors", 0) and tinfo.get("num_turns", 0) <= 1 and not tinfo.get("edits", {}).get("total"):
        return "API errors before any work"
    return None

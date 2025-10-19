#!/usr/bin/env python3
"""
Feature Doc & Token Aggregator (with logging)

- Recursively scans all .rs files (skips common build/hidden dirs)
- Summarizes each file (~4000-token budget via ~12k chars)
- Calls a local LLM (LM Studio) to:
  1) score ALL features and pick a primary
  2) emit compact tokens for EVERY sufficiently matching feature
- Aggregates tokens globally and generates per-feature docs

Now includes detailed logging:
- Logs every HTTP error (status + truncated body)
- Logs JSON parse failures (with sample)
- Tracks per-file failures and prints a summary at the end
"""

import os
import re
import sys
import json
import time
import textwrap
import hashlib
import logging
from pathlib import Path
from typing import Dict, List, Tuple, Optional

# --------------------------
# Logging setup
# --------------------------
LOG_LEVEL = logging.INFO  # change to logging.DEBUG for more noise
logging.basicConfig(
    level=LOG_LEVEL,
    format="%(asctime)s | %(levelname)s | %(message)s",
    datefmt="%H:%M:%S",
)
logger = logging.getLogger("gen")

# Track failures for end-of-run summary
FAILED_CALLS: List[dict] = []

# Progress tracking (set in main)
TOTAL_FILES: int = 0
CURRENT_FILE_IDX: int = 0

# --------------------------
# Fixed model & behavior (no env)
# --------------------------
# LM server base URL (can be overridden with LM_BASE env var). The script will try a few
# common endpoints used by LM Studio / local serving layers (chat/completions and generate).
# Hardcoded LMStudio local server settings (per user request: no env vars)
LM_BASE  = "http://127.0.0.1:1234"
LM_MODEL = "qwen2.5-coder-7b-instruct"
TEMPERATURE = 0.0

# Multi-feature token aggregation knobs
SCORE_MIN = 0.55   # include tokens for any feature with score >= SCORE_MIN
TOP_K_FALLBACK = 2 # ensure at least K features per file get tokens

# Context budget: ~4000 tokens default → ~12k chars (conservative)
MAX_SUMMARY_CHARS = 12000

# Output & cache
ROOT      = Path(os.getcwd()).resolve()
OUT_DIR   = ROOT / "docs" / "project-index"
CACHE_DIR = ROOT / ".cache"
CACHE_DIR.mkdir(exist_ok=True)
CLASSIFY_CACHE_FILE = CACHE_DIR / "feature_map_cache.json"   # per-file class scores + primary
TOKENS_DB_FILE      = OUT_DIR / "feature_tokens.json"        # global token index

# Files/dirs walking
IGNORE_DIRS = {
    ".git", ".hg", ".svn", ".idea", ".vscode", "target", "node_modules", "build", "dist", "out"
}
INCLUDE_EXT = {".rs"}

# Public API line detection (for summaries / API lists)
PUB_ITEM_RE = re.compile(r"(?m)^(pub\s+(?:struct|enum|trait|fn)\s+[A-Za-z0-9_]+)")

# --------------------------
# Fixed feature set (no paths/regex) — edit only when you add/remove features
# --------------------------
TAXONOMY_VERSION = 2
FEATURES: List[Dict] = [
    {"slug": "snapshots", "title": "All About Snapshots",
     "prompt": "Snapshot system: isolated/temporary views of mutable state; nested snapshots; apply/merge; change tracking; observers."},
    {"slug": "state", "title": "State & Ownership",
     "prompt": "User-facing state primitives and ownership; remember/useState patterns; State<T> semantics; observation/update flow."},
    {"slug": "composer_runtime", "title": "Composer & Runtime",
     "prompt": "Composer, recomposition engine, slot tables, groups, invalidation, applier; composition lifecycle."},
    {"slug": "effects", "title": "Effects",
     "prompt": "SideEffect, DisposableEffect, LaunchedEffect; lifecycles; cleanup/cancel semantics; integration with runtime/frame or async context."},
    {"slug": "frame_clock", "title": "Frame Clock",
     "prompt": "Frame time APIs and scheduling; with_frame_nanos/millis; draining frame callbacks; time-driven recomposition/animation."},
    {"slug": "layout", "title": "Layout System",
     "prompt": "Measure/place contracts, constraints, intrinsic measurements, Measurable/Placeable; multi-pass layout logic."},
    {"slug": "modifiers", "title": "Modifiers",
     "prompt": "Modifier chain and nodes; parameter reuse; on_attach/on_detach; LayoutModifierNode; bridging to layout/rendering/input."},
    {"slug": "rendering", "title": "Rendering",
     "prompt": "Renderer backends, draw routines, GPU/CPU pipelines; wgpu/pixels adapters; render tree traversal."},
    {"slug": "animation", "title": "Animation",
     "prompt": "Time-based value interpolation; tween/spring; Animatable; frame clock integration; cancellation."},
    {"slug": "composition_local", "title": "Composition Local",
     "prompt": "Ambient values via CompositionLocal; providers and lookups; scoping and read observation."},
    {"slug": "input", "title": "Input",
     "prompt": "Pointer/keyboard/focus input plumbing; hit testing; event dispatch; focus traversal."},
    {"slug": "platform_desktop", "title": "Desktop Platform (winit)",
     "prompt": "Desktop integration (winit/event loop/windowing), app lifecycle glue for desktop targets."},
    {"slug": "app_shell", "title": "App Shell",
     "prompt": "Top-level application loop and integration glue between platform and runtime."},
    {"slug": "widgets", "title": "Widgets",
     "prompt": "User-visible UI primitives: Row/Column/Box/Text/Button/Spacer etc."},
    {"slug": "subcompose", "title": "Subcompose",
     "prompt": "Subcompose layouts/scopes; deferred composition and measure-time composition."},
    {"slug": "diagnostics", "title": "Diagnostics",
     "prompt": "Debug/tracing/inspection utilities; runtime instrumentation."},
]

# Quick slug->title map for docs (feature_map will no longer store titles)
FEATURE_TITLES = {f["slug"]: f.get("title", f["slug"]) for f in FEATURES}

# ==========================
# Helpers
# ==========================

def sha1_bytes(b: bytes) -> str:
    return hashlib.sha1(b).hexdigest()

def list_rust_files(root: Path) -> List[Path]:
    files: List[Path] = []
    for dirpath, dirnames, filenames in os.walk(root):
        dirnames[:] = [d for d in dirnames if d not in IGNORE_DIRS and not d.startswith(".")]
        for fn in filenames:
            p = Path(dirpath) / fn
            if p.suffix in INCLUDE_EXT:
                files.append(p)
    return files

def read_text(path: Path) -> str:
    try:
        return path.read_text(encoding="utf-8")
    except UnicodeDecodeError:
        return path.read_text(encoding="utf-8", errors="ignore")

def extract_public_items(code: str, limit: int = 64) -> List[str]:
    items = [m for m in PUB_ITEM_RE.findall(code)]
    return items[:limit]

def extract_leading_docs(code: str, max_lines: int = 120) -> str:
    lines = code.splitlines()
    collected = []
    opened_block = False
    for i, ln in enumerate(lines[:max_lines]):
        s = ln.strip()
        if s.startswith("//!") or s.startswith("///") or s.startswith("//"):
            collected.append(ln)
            continue
        if s.startswith("/*"):
            opened_block = True
            collected.append(ln)
            continue
        if opened_block:
            collected.append(ln)
            if "*/" in s:
                opened_block = False
            continue
        collected.extend(lines[i:i+20])
        break
    return "\n".join(collected[:max_lines])

def truncate_chars(s: str, limit: int) -> str:
    if len(s) <= limit:
        return s
    head = s[:limit]
    last_nl = head.rfind("\n")
    return head[:last_nl] if last_nl > limit * 0.8 else head


def summarize_for_llm(path: str, code: str) -> str:
    leading = extract_leading_docs(code, max_lines=160)
    apis = extract_public_items(code, limit=64)
    apis_block = "- " + "\n- ".join(apis) if apis else "- (no public items found)"
    body_tail = "\n".join(code.splitlines()[-80:])
    raw = f"""PATH (for context only): {path}

PUBLIC_APIS:
{apis_block}

HEAD_DOCS_OR_CODE:
{leading}

CODE_TAIL:
{body_tail}
"""
    return truncate_chars(raw, MAX_SUMMARY_CHARS)

# ==========================
# HTTP / LLM helpers with logging + retry
# ==========================

def _shorten(s: str, n: int = 800) -> str:
    s = s or ""
    return (s[:n] + "…") if len(s) > n else s

def http_json_chat(payload: Dict, phase: str, relpath: str, tries: int = 2, backoff_sec: float = 0.8) -> Optional[Dict]:
    """
    POSTs to LM_URL and returns parsed JSON (requests JSON, not model content).
    On failure, logs details and returns None.
    Retries a couple of times with simple backoff.
    """
    import requests
    # include current progress in logs if available
    try:
        progress = f"[{CURRENT_FILE_IDX}/{TOTAL_FILES}] " if TOTAL_FILES else ""
    except Exception:
        progress = ""

    # Use only OpenAI-like supported endpoints to avoid LM server warnings.
    # We'll try chat completions first, then fallback to non-chat completions
    # (converted from messages -> prompt).
    endpoints = [
        f"{LM_BASE.rstrip('/')}/v1/chat/completions",
        f"{LM_BASE.rstrip('/')}/v1/completions",
    ]

    # Helper to convert messages->single prompt for generate endpoints
    def messages_to_prompt(msgs: List[Dict]) -> str:
        parts = []
        for m in msgs:
            role = m.get("role", "user")
            content = m.get("content", "")
            parts.append(f"[{role}]\n" + content)
        return "\n\n".join(parts)

    last_exc = None
    for attempt in range(1, tries + 1):
        for url in endpoints:
            try:
                # For /v1/completions (non-chat) convert messages -> prompt.
                if url.endswith("/completions") and "/chat/" not in url:
                    msgs = payload.get("messages") or []
                    prompt = messages_to_prompt(msgs) if msgs else payload.get("prompt") or ""
                    send = {"model": payload.get("model", LM_MODEL), "prompt": prompt, "temperature": payload.get("temperature", 0.0)}
                else:
                    send = payload

                logger.info("%sLLM call %s -> %s | url=%s", progress, phase, relpath, url)
                r = requests.post(url, json=send, timeout=120)
                if r.status_code != 200:
                    logger.error("%sAPI %s FAILED (%s) for %s | url=%s | status=%s | body=%s",
                                 progress, phase, attempt, relpath, url, r.status_code, _shorten(r.text))
                    FAILED_CALLS.append({"phase": phase, "file": relpath, "url": url, "status": r.status_code, "body": r.text[:4000]})
                    time.sleep(backoff_sec * attempt)
                    continue
                try:
                    logger.debug("API %s SUCCESS for %s | url=%s | status=%s", phase, relpath, url, r.status_code)
                    return r.json()
                except Exception as je:
                    logger.error("%sAPI %s JSON PARSE FAILED for %s | url=%s | err=%s | body=%s",
                                 progress, phase, relpath, url, je, _shorten(r.text))
                    FAILED_CALLS.append({"phase": phase, "file": relpath, "url": url, "status": r.status_code, "body": r.text[:4000]})
                    time.sleep(backoff_sec * attempt)
            except Exception as e:
                last_exc = e
                logger.error("%sAPI %s EXCEPTION for %s | url=%s | err=%s", progress, phase, relpath, url, e)
                FAILED_CALLS.append({"phase": phase, "file": relpath, "url": url, "status": None, "body": str(e)})
                time.sleep(backoff_sec * attempt)

    if last_exc:
        logger.debug("http_json_chat last exception: %s", last_exc)
    return None


def _extract_json_blob(s: str) -> str:
    """Try to extract a JSON object from model content.

    Handles cases where the assistant wraps JSON in markdown fences (```json ... ```)
    or includes surrounding commentary. Returns original string if no JSON found.
    """
    if not s:
        return s
    # Strip common triple-backtick fences
    # Look for ```json ... ``` or ``` ... ```
    m = re.search(r"```(?:json)?\s*(.*?)\s*```", s, re.S | re.I)
    if m:
        return m.group(1).strip()

    # Fallback: find first { and the matching last }
    first = s.find("{")
    last = s.rfind("}")
    if first != -1 and last != -1 and last > first:
        return s[first:last+1]

    # Nothing obvious — return original to allow json.loads to raise a helpful error
    return s


def probe_available_model() -> Optional[str]:
    """Query the server's /v1/models and return the first model id if successful.

    Returns None on failure.
    """
    import requests
    url = f"{LM_BASE.rstrip('/')}/v1/models"
    try:
        r = requests.get(url, timeout=6)
        if r.status_code != 200:
            logger.debug("Model probe failed status=%s body=%s", r.status_code, _shorten(r.text))
            return None
        data = r.json()
        arr = data.get("data") or []
        if not arr:
            return None
        first = arr[0].get("id")
        return first
    except Exception as e:
        logger.debug("Model probe exception: %s", e)
        return None

# ==========================
# LLM Calls (with logging)
# ==========================

def lm_score_features(features: List[Dict], file_summary: str, relpath: str) -> Dict:
    system = {"role": "system", "content": textwrap.dedent("""
        You classify Rust source content into a fixed feature set for a Jetpack-Compose-like framework.
        - Use ONLY the provided summary (APIs + docs/tail). Ignore file paths for decision.
        - Score EVERY feature from 0.0 to 1.0 by fitness.
        - Select ONE primary feature with a confidence (0..1).
        Return strict JSON: {"scores":{"<slug>":0.0..1.0,...},"primary":"<slug>","confidence":0.0..1.0}
    """).strip()}
    feat_lines = [f"- {f['slug']}: {f['title']} || What to look for: {f.get('prompt','')}" for f in features]
    user = {"role": "user", "content": "Allowed features:\n" + "\n".join(feat_lines) + "\n\nFILE SUMMARY:\n" + file_summary}
    payload = {"model": LM_MODEL, "temperature": TEMPERATURE, "messages": [system, user]}

    logger.debug("Scoring features for %s", relpath)
    data = http_json_chat(payload, phase="score_features", relpath=relpath)
    if data is None:
        return {"scores": {}, "primary": "unknown", "confidence": 0.0}

    content = data.get("choices", [{}])[0].get("message", {}).get("content", "")
    clean = _extract_json_blob(content)
    try:
        return json.loads(clean)
    except Exception as je:
        logger.error("Content JSON parse failed (score_features) for %s | err=%s | content=%s",
                     relpath, je, _shorten(content))
        FAILED_CALLS.append({"phase": "score_features_content_parse", "file": relpath,
                             "status": None, "body": _shorten(content, 1200)})
        return {"scores": {}, "primary": "unknown", "confidence": 0.0}

def lm_tokens_for_features(features: List[Dict], candidate_slugs: List[str], file_summary: str, relpath: str) -> Dict[str, List[Dict]]:
    if not candidate_slugs:
        return {}
    cards = []
    for s in candidate_slugs:
        f = next((x for x in features if x["slug"] == s), None)
        if f:
            cards.append({"slug": f["slug"], "title": f["title"], "what": f.get("prompt", "")})
    system = {"role": "system", "content":
              "Generate compact, code-friendly tokens for EACH requested feature from a Rust file's APIs/docs. "
              "For each feature, output 8–20 tokens (1–3 words). Prefer public type/function names and key terms. "
              "Return strict JSON: {\"features\": {\"<slug>\": [{\"token\":\"...\",\"weight\":0..1}, ...], ...}}"}
    user = {"role": "user", "content": "FEATURES:\n" +
            "\n".join([f"- {c['slug']}: {c['title']} — {c['what']}" for c in cards]) +
            "\n\nFILE SUMMARY:\n" + file_summary}
    payload = {"model": LM_MODEL, "temperature": 0.0, "messages": [system, user]}

    logger.debug("Requesting tokens for %s | features=%s", relpath, ",".join(candidate_slugs))
    data = http_json_chat(payload, phase="tokens_for_features", relpath=relpath)
    if data is None:
        return {}

    content = data.get("choices", [{}])[0].get("message", {}).get("content", "")
    clean = _extract_json_blob(content)
    try:
        raw = json.loads(clean)
    except Exception as je:
        logger.error("Content JSON parse failed (tokens_for_features) for %s | err=%s | content=%s",
                     relpath, je, _shorten(content))
        FAILED_CALLS.append({"phase": "tokens_for_features_content_parse", "file": relpath,
                             "status": None, "body": _shorten(content, 1200)})
        return {}

    out: Dict[str, List[Dict]] = {}
    for slug, arr in raw.get("features", {}).items():
        norm = []
        for t in arr:
            if isinstance(t, str):
                norm.append({"token": t, "weight": 0.6})
            elif isinstance(t, dict):
                norm.append({"token": str(t.get("token", "")), "weight": float(t.get("weight", 0.6))})
        out[slug] = norm
    return out

# ==========================
# Caching
# ==========================

def load_classify_cache() -> Dict:
    if CLASSIFY_CACHE_FILE.exists():
        try:
            return json.loads(CLASSIFY_CACHE_FILE.read_text())
        except Exception:
            return {}
    return {}

def save_classify_cache(c: Dict):
    CLASSIFY_CACHE_FILE.write_text(json.dumps(c, indent=2, ensure_ascii=False))

# ==========================
# Token Index (GLOBAL)
# ==========================

GENERIC_TOKENS = {"state","runtime","system","value","data","item","node","layout","render","function","struct","trait"}

def _norm_token(t: str) -> str:
    t = t.strip().lower()
    t = re.sub(r"[^a-z0-9_+\-/\. ]+", "", t)
    t = re.sub(r"\s+", " ", t)
    return t[:64]

def _valid_token(tok: str) -> bool:
    if not tok or len(tok) < 2:
        return False
    if tok in GENERIC_TOKENS:
        return False
    return True

def load_tokens_db() -> Dict:
    if TOKENS_DB_FILE.exists():
        try:
            raw = json.loads(TOKENS_DB_FILE.read_text())
            # If file already contains the internal token_index, return it
            if isinstance(raw, dict) and raw.get("token_index"):
                return raw

            # If this is the old 'features' -> {slug: {tokens: [...]}} format, convert.
            if isinstance(raw, dict) and raw.get("features"):
                db = {"taxonomy_version": raw.get("taxonomy_version"), "model": raw.get("model"), "token_index": {}}
                for slug, meta in (raw.get("features") or {}).items():
                    toks = meta.get("tokens") or []
                    for t in toks:
                        tok = _norm_token(t)
                        if not _valid_token(tok):
                            continue
                        ent = db["token_index"].setdefault(tok, {
                            "global": {"count": 0, "weight_sum": 0.0, "examples": [], "last_seen": 0},
                            "by_feature": {}
                        })
                        ent["global"]["count"] += 1
                        ent["global"]["weight_sum"] += 1.0
                        fe = ent["by_feature"].setdefault(slug, {"count": 0, "weight_sum": 0.0})
                        fe["count"] += 1
                        fe["weight_sum"] += 1.0
                return db

            # If this is the flat token -> [paths] map (with optional taxonomy_version/model),
            # convert to internal token_index. Detect by presence of string keys mapping to lists
            # excluding known meta keys.
            if isinstance(raw, dict):
                known = {"taxonomy_version", "model"}
                token_keys = [k for k, v in raw.items() if k not in known and isinstance(v, list)]
                if token_keys:
                    db = {"taxonomy_version": raw.get("taxonomy_version"), "model": raw.get("model"), "token_index": {}}
                    now = int(time.time())
                    for tok in token_keys:
                        norm = _norm_token(tok)
                        if not _valid_token(norm):
                            continue
                        paths = raw.get(tok) or []
                        ent = db["token_index"].setdefault(norm, {
                            "global": {"count": 0, "weight_sum": 0.0, "examples": [], "last_seen": 0},
                            "by_feature": {}
                        })
                        ent["global"]["count"] = len(paths)
                        ent["global"]["weight_sum"] = float(len(paths))
                        ent["global"]["examples"] = list(dict.fromkeys(paths))[:6]
                        ent["global"]["last_seen"] = now
                    return db
        except Exception:
            pass
    return {"taxonomy_version": None, "model": None, "token_index": {}}

def save_tokens_db(db: Dict):
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    # Produce a flat token -> [paths] map (plus taxonomy_version and model keys)
    try:
        idx = db.get("token_index", {})
        token_map = {}
        for tok, meta in idx.items():
            examples = meta.get("global", {}).get("examples", [])
            if examples:
                token_map[tok] = list(dict.fromkeys(examples))

        out = {"taxonomy_version": db.get("taxonomy_version"), "model": db.get("model")}
        out.update(token_map)
        TOKENS_DB_FILE.write_text(json.dumps(out, indent=2, ensure_ascii=False))
    except Exception:
        TOKENS_DB_FILE.write_text(json.dumps(db, indent=2, ensure_ascii=False))

def merge_token_index(db: Dict, taxonomy_version: int, model_name: str,
                      slug: str, file_path: str, suggestions: List[Dict]):
    db["taxonomy_version"] = taxonomy_version
    db["model"] = model_name
    idx = db.setdefault("token_index", {})
    now = int(time.time())
    for s in suggestions:
        tok = _norm_token(s.get("token", ""))
        w = float(s.get("weight", 0.6))
        if not _valid_token(tok):
            continue
        ent = idx.setdefault(tok, {
            "global": {"count": 0, "weight_sum": 0.0, "examples": [], "last_seen": 0},
            "by_feature": {}
        })
        ent["global"]["count"] += 1
        ent["global"]["weight_sum"] += w
        ent["global"]["last_seen"] = now
        if len(ent["global"]["examples"]) < 6 and file_path not in ent["global"]["examples"]:
            ent["global"]["examples"].append(file_path)
        fe = ent["by_feature"].setdefault(slug, {"count": 0, "weight_sum": 0.0})
        fe["count"] += 1
        fe["weight_sum"] += w

def top_tokens_for_feature(db: Dict, slug: str, k: int = 32) -> List[str]:
    idx = db.get("token_index", {})
    scored = []
    for tok, meta in idx.items():
        if slug not in meta.get("by_feature", {}):
            continue
        fe = meta["by_feature"][slug]
        avg_w = fe["weight_sum"] / max(1, fe["count"])
        score = fe["count"] * (0.5 + 0.5 * avg_w)
        scored.append((score, tok))
    scored.sort(reverse=True)
    return [tok for _, tok in scored[:k]]

def tokens_global_sorted(db: Dict) -> List[Tuple[str, Dict]]:
    idx = db.get("token_index", {})
    def gscore(meta):
        g = meta["global"]
        avg = g["weight_sum"] / max(1, g["count"])
        return g["count"] * (0.5 + 0.5 * avg)
    return sorted(idx.items(), key=lambda kv: gscore(kv[1]), reverse=True)

# ==========================
# Generation (docs)
# ==========================

def write_feature_map_json(feature_map: Dict):
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    # Write a slim feature map: slug -> list of files (no titles, no nesting)
    slim = {slug: meta.get("include", []) for slug, meta in feature_map.items()}
    (OUT_DIR / "feature_map.json").write_text(
        json.dumps({"taxonomy_version": TAXONOMY_VERSION, "features": slim}, indent=2),
        encoding="utf-8"
    )

def write_tokens_md(tokens_db: Dict):
    # TOKENS.md generation disabled per user request (no op)
    return

def write_feature_docs(feature_map: Dict, file_code_map: Dict[str, str], tokens_db: Dict):
    for slug, meta in feature_map.items():
        paths = sorted(set(meta.get("include", [])))
        if not paths:
            continue
        title = FEATURE_TITLES.get(slug, slug)
        parts = [f"# {title}\n", f"> Purpose: {title}.\n", "## Primary APIs\n"]
        apis = []
        for p in paths:
            code = file_code_map.get(p, "")
            apis.extend(m for m in PUB_ITEM_RE.findall(code))
        apis = sorted(set(apis))[:120]
        parts += [*(f"- `{a}`" for a in apis)] or ["- _(none)_"]

        parts.append("\n## Discovered tokens (aggregated)\n")
        tokens = top_tokens_for_feature(tokens_db, slug, k=32)
        if tokens:
            parts += [*(f"- `{t}`" for t in tokens)]
        else:
            parts.append("- _(none yet)_")

        parts.append("\n## How it works\n<!-- @editable:start -->\n<!-- add notes -->\n<!-- @editable:end -->\n")
        parts.append("## Source Files (excerpts)\n")
        for p in paths:
            code = file_code_map.get(p, "")
            lead = extract_leading_docs(code, max_lines=80)
            items = extract_public_items(code, limit=12)
            parts.append(f"### {p}\n")
            if items:
                parts.append("**Public APIs (subset):**")
                for it in items:
                    parts.append(f"- `{it}`")
                parts.append("")
            parts.append("```rust")
            parts.append(lead.rstrip())
            parts.append("```")
            parts.append("")
        (OUT_DIR / f"all_about_{slug}.md").write_text("\n".join(parts), encoding="utf-8")

def write_index(feature_map: Dict, tokens_db: Dict):
    # README.md/index generation disabled per user request (no op)
    return

# ==========================
# Main
# ==========================

def main():
    OUT_DIR.mkdir(parents=True, exist_ok=True)

    # Simple CLI
    import argparse
    ap = argparse.ArgumentParser(description="Generate feature docs with local LLM")
    ap.add_argument("--no-cache", action="store_true", help="Ignore classification cache and force LLM calls")
    ap.add_argument("--run-tests", action="store_true", help="Run `cargo test` after generation (opt-in). Tests are skipped by default.")
    args = ap.parse_args()

    # Probe the server for available models and pick the first if our model isn't found
    try:
        found = probe_available_model()
        if found:
            logger.info("Model probe: first available model -> %s", found)
            # If our configured model isn't exactly the probed one, switch to the probed model
            if LM_MODEL != found:
                logger.info("Switching LM_MODEL from %s to %s (probe)", LM_MODEL, found)
                # mutate the global
                globals()["LM_MODEL"] = found
    except Exception:
        pass

    # Walk repo for .rs files
    rs_files = list_rust_files(ROOT)
    if not rs_files:
        logger.warning("No .rs files found.")
        return

    # Load caches/DB
    classify_cache = load_classify_cache()
    classify_cache.setdefault("taxonomy_version", TAXONOMY_VERSION)
    classify_cache.setdefault("files", {})
    if args.no_cache:
        logger.info("--no-cache: clearing classification cache and tokens DB; forcing LLM calls")
        classify_cache = {"taxonomy_version": TAXONOMY_VERSION, "files": {}}
        try:
            if CLASSIFY_CACHE_FILE.exists():
                CLASSIFY_CACHE_FILE.unlink()
        except Exception:
            pass

    if classify_cache["taxonomy_version"] != TAXONOMY_VERSION:
        classify_cache["files"] = {}
        classify_cache["taxonomy_version"] = TAXONOMY_VERSION
        logger.info("Taxonomy version changed → invalidated classification cache.")

    tokens_db = load_tokens_db()

    # Initialize feature map (for primary assignments)
    feature_map: Dict[str, Dict] = {f["slug"]: {"title": f["title"], "include": []} for f in FEATURES}
    feature_map.setdefault("unknown", {"title": "Unclassified", "include": []})

    file_code_map: Dict[str, str] = {}

    # Process each file
    global TOTAL_FILES, CURRENT_FILE_IDX
    TOTAL_FILES = len(rs_files)
    for idx, path in enumerate(rs_files, 1):
        CURRENT_FILE_IDX = idx
        logger.info("Processing file %d/%d: %s", idx, TOTAL_FILES, str(path.relative_to(ROOT)))
        rel = str(path.relative_to(ROOT))
        code = read_text(path)
        file_code_map[rel] = code
        file_hash = sha1_bytes(code.encode("utf-8"))

        cached = classify_cache["files"].get(rel)
        if cached and cached.get("sha1") == file_hash:
            primary = cached["primary"]
            scores = cached["scores"]
            logger.debug("[%d/%d] reuse cache: %s → %s", idx, len(rs_files), rel, primary)
        else:
            summary = summarize_for_llm(rel, code)
            logger.debug("[%d/%d] scoring: %s", idx, len(rs_files), rel)
            resp = lm_score_features(FEATURES, summary, rel)
            primary = resp.get("primary", "unknown")
            scores  = resp.get("scores", {})
            classify_cache["files"][rel] = {"sha1": file_hash, "primary": primary, "scores": scores}
            logger.info("[%d/%d] primary=%s | %s", idx, len(rs_files), primary, rel)

        # Candidate features for token emission (primary or not)
        ranked = sorted(scores.items(), key=lambda kv: kv[1], reverse=True)
        candidates = [s for s, v in ranked if v >= SCORE_MIN]
        if len(candidates) < min(TOP_K_FALLBACK, len(ranked)):
            candidates = [s for s, _ in ranked[:TOP_K_FALLBACK]]

        # Assign file to all matching features (allow shared files). If no candidates
        # were selected, fall back to assigning to the primary feature.
        if candidates:
            for s in candidates:
                feature_map.setdefault(s, {"title": s, "include": []})
                if rel not in feature_map[s]["include"]:
                    feature_map[s]["include"].append(rel)
        else:
            feature_map.setdefault(primary, {"title": primary, "include": []})
            if rel not in feature_map[primary]["include"]:
                feature_map[primary]["include"].append(rel)

        # Generate tokens for ALL candidate features
        if candidates:
            summary = summarize_for_llm(rel, code)
            toks_by_feat = lm_tokens_for_features(FEATURES, candidates, summary, rel)
            for slug, toks in toks_by_feat.items():
                merge_token_index(tokens_db, TAXONOMY_VERSION, LM_MODEL, slug, rel, toks)
                # Show generated tokens immediately in progress logs (limit to first 12)
                try:
                    preview = []
                    for t in (toks or [])[:12]:
                        if isinstance(t, dict):
                            tok = str(t.get("token", "")).strip()
                            w = float(t.get("weight", 0.0))
                            preview.append(f"{tok}({w:.2f})")
                        else:
                            preview.append(str(t).strip())
                    logger.info("[%d/%d] tokens -> %s: %s", idx, TOTAL_FILES, slug, ", ".join(preview) or "(none)")
                except Exception as e:
                    logger.debug("[%d/%d] tokens preview failed for %s: %s", idx, TOTAL_FILES, slug, e)
        else:
            logger.debug("No candidate features above threshold for %s", rel)

        # Persist incremental progress after each file so partial results are available on-disk.
        try:
            save_classify_cache(classify_cache)
            save_tokens_db(tokens_db)
            write_feature_map_json(feature_map)
            logger.info("[%d/%d] saved progress to: %s", idx, TOTAL_FILES, OUT_DIR)
        except Exception as e:
            logger.debug("[%d/%d] failed to save progress for %s: %s", idx, TOTAL_FILES, rel, e)

    # Persist artifacts
    save_classify_cache(classify_cache)
    save_tokens_db(tokens_db)
    write_feature_map_json(feature_map)
    write_feature_docs(feature_map, file_code_map, tokens_db)

    # Final summary
    print(f"Processed {len(rs_files)} Rust files.")
    print(f"Outputs in: {OUT_DIR}")

    if FAILED_CALLS:
        print("\n--- LLM API failures / parse issues ---")
        by_phase: Dict[str, int] = {}
        for rec in FAILED_CALLS:
            by_phase[rec["phase"]] = by_phase.get(rec["phase"], 0) + 1
        for phase, count in sorted(by_phase.items()):
            print(f"{phase}: {count}")
        # Optionally dump a JSON for debugging
        (OUT_DIR / "llm_failures.json").write_text(json.dumps(FAILED_CALLS, indent=2, ensure_ascii=False))
        print(f"Details: {OUT_DIR}/llm_failures.json")
    else:
        print("LLM API: all calls succeeded.")
    # Optionally run tests (opt-in flag)
    if args.run_tests:
        import subprocess
        try:
            logger.info("--run-tests: running 'cargo test' (this may take a while)")
            subprocess.run(["cargo", "test"], check=True)
        except Exception as e:
            logger.error("cargo test failed: %s", e)


if __name__ == "__main__":
    main()

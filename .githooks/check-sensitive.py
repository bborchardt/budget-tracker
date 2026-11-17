#!/usr/bin/env python3
"""Block commits and pushes that contain personal or financial data.

Used by .githooks/pre-commit and .githooks/pre-push. See AGENTS.md.

Usage:
  check-sensitive.py commit   # scan staged changes
  check-sensitive.py push     # scan commits being pushed (reads pre-push stdin)
  check-sensitive.py range BASE HEAD   # scan commits in BASE..HEAD (used by CI)

Findings report file, line, and rule only. Matched text is never printed,
so the check itself doesn't echo sensitive data into terminals or logs.

To allow a known-safe line (e.g., a fake value in a synthetic test fixture),
add the marker `pii-allow` on that line. Use sparingly and only with the
project owner's approval.
"""
import re
import subprocess
import sys

ALLOW_MARKER = "pii-allow"
FIXTURE_PREFIX = "tests/fixtures/"
ZERO_SHA = "0" * 40
SQLITE_MAGIC = b"SQLite format 3\x00"

BLOCKED_PATHS = [
    (re.compile(r"\.(db|sqlite|sqlite3|db-journal)$", re.I), "database file"),
    (re.compile(r"-(wal|shm|journal)$", re.I), "SQLite side file"),
    (re.compile(r"\.(ofx|qfx|qif|xlsx|xls|bak|backup)$", re.I), "bank/app export or backup"),
    (re.compile(r"(^|/)\.env(\.[^/]*)?$"), "environment/secrets file"),
    (re.compile(r"\.(pem|key|p12|pfx)$", re.I), "key or certificate"),
    (re.compile(r"(^|/)\.claude/settings\.local\.json$"), "local tool settings"),
]
DATA_FILE = re.compile(r"\.(csv|tsv)$", re.I)


def luhn_ok(digits: str) -> bool:
    total = 0
    for i, ch in enumerate(reversed(digits)):
        d = int(ch)
        if i % 2 == 1:
            d *= 2
            if d > 9:
                d -= 9
        total += d
    return total % 10 == 0


def card_number(line: str) -> bool:
    # Digit runs inside a longer letters-and-digits token (hex checksums, hashes, IDs) are skipped:
    # a real card number stands alone or is grouped with spaces/dashes.
    for m in re.finditer(r"(?<![\dA-Za-z.])(?:\d[ -]?){12,18}\d(?![\dA-Za-z])", line):
        digits = re.sub(r"\D", "", m.group())
        if 13 <= len(digits) <= 19 and luhn_ok(digits):
            return True
    return False


def regex(pattern, flags=0):
    compiled = re.compile(pattern, flags)
    return lambda line: compiled.search(line) is not None


EMAIL = re.compile(r"[A-Za-z0-9._%+-]+@(?!example\.(?:com|org|net)\b)[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)*\.([A-Za-z]{2,})")
# Filenames like "icon@2x.png" look like emails; these endings are file types, not email domains.
FILE_EXTENSIONS = {"png", "jpg", "jpeg", "gif", "svg", "webp", "ico", "icns", "bmp", "tiff",
                   "json", "css", "js", "html", "toml", "yaml", "yml", "lock", "svelte", "rs"}


def email_address(line: str) -> bool:
    return any(m.group(1).lower() not in FILE_EXTENSIONS for m in EMAIL.finditer(line))


CONTENT_RULES = [
    ("possible card number (passes Luhn check)", card_number),
    ("masked card/account number (stars, x's, or 'ending in' followed by 4 digits)",
     regex(r"(?:\*{2,}|[xX]{3,}|\.{3}|\bending in\s*)\d{4}\b", re.I)),
    ("labeled account/routing number",
     regex(r"\b(?:acct|account|routing|aba)\s*(?:no\.?|number|num|#)?\s*[:#]?\s*\d{4,}", re.I)),
    ("SSN-like number", regex(r"\b\d{3}-\d{2}-\d{4}\b")),
    ("email address (only @example.com/.org/.net allowed)", email_address),
    ("API key or secret",
     regex(r"\bsk-[A-Za-z0-9_-]{16,}|\bAKIA[0-9A-Z]{16}\b|-----BEGIN [A-Z ]*PRIVATE KEY-----"
           r"|\b(?:api[_-]?key|secret|token|password)\s*[:=]\s*[\"']?[A-Za-z0-9_\-]{12,}", re.I)),
]


def git(*args: str) -> bytes:
    return subprocess.run(["git", *args], check=True, capture_output=True).stdout


def check_paths(paths, read_blob, where, findings):
    for path in paths:
        for pattern, reason in BLOCKED_PATHS:
            if pattern.search(path):
                findings.append(f"{where}{path}: blocked file type ({reason})")
        if DATA_FILE.search(path) and not path.startswith(FIXTURE_PREFIX):
            findings.append(f"{where}{path}: data file outside {FIXTURE_PREFIX} (synthetic fixtures only)")
        try:
            if read_blob(path).startswith(SQLITE_MAGIC):
                findings.append(f"{where}{path}: file contents are a SQLite database")
        except subprocess.CalledProcessError:
            pass


def check_diff(diff_text, where, findings):
    path, line_no = None, 0
    for raw in diff_text.splitlines():
        if raw.startswith("+++ "):
            target = raw[4:]
            path = target[2:] if target.startswith("b/") else None
            continue
        if raw.startswith("@@"):
            m = re.search(r"\+(\d+)", raw)
            line_no = int(m.group(1)) if m else 0
            continue
        if path is None or not raw.startswith("+"):
            continue
        line = raw[1:]
        if ALLOW_MARKER not in line:
            for reason, test in CONTENT_RULES:
                if test(line):
                    findings.append(f"{where}{path}:{line_no}: {reason}")
        line_no += 1


def changed_paths(args):
    out = git(*args, "--name-only", "-z", "--diff-filter=ACMR")
    return [p for p in out.decode("utf-8", "replace").split("\0") if p]


def scan_staged(findings):
    paths = changed_paths(["diff", "--cached", "--no-renames"])
    check_paths(paths, lambda p: git("show", f":{p}")[:16], "", findings)
    diff = git("diff", "--cached", "--no-renames", "-U0", "--no-color", "--no-ext-diff",
               "--diff-filter=ACMR").decode("utf-8", "replace")
    check_diff(diff, "", findings)


def scan_commits(revs, findings):
    for commit in revs.decode().split():
        where = f"[{commit[:8]}] "
        base = ["diff-tree", "-r", "--root", "--no-commit-id", "--no-renames"]
        paths = changed_paths([*base, commit])
        check_paths(paths, lambda p, c=commit: git("show", f"{c}:{p}")[:16], where, findings)
        diff = git(*base, "-p", "-U0", "--no-color", "--no-ext-diff",
                   "--diff-filter=ACMR", commit).decode("utf-8", "replace")
        check_diff(diff, where, findings)


def scan_push(findings):
    for entry in sys.stdin.read().splitlines():
        parts = entry.split()
        if len(parts) != 4:
            continue
        _local_ref, local_sha, _remote_ref, remote_sha = parts
        if local_sha == ZERO_SHA:
            continue  # deleting a remote branch
        if remote_sha == ZERO_SHA:
            revs = git("rev-list", local_sha, "--not", "--remotes")
        else:
            revs = git("rev-list", f"{remote_sha}..{local_sha}")
        scan_commits(revs, findings)


def scan_range(base_sha, head_sha, findings):
    if not base_sha or base_sha == ZERO_SHA:
        revs = git("rev-list", head_sha)
    else:
        revs = git("rev-list", f"{base_sha}..{head_sha}")
    scan_commits(revs, findings)


def main():
    mode = sys.argv[1] if len(sys.argv) > 1 else "commit"
    findings = []
    if mode == "push":
        scan_push(findings)
    elif mode == "range":
        if len(sys.argv) != 4:
            print("usage: check-sensitive.py range BASE HEAD", file=sys.stderr)
            return 2
        scan_range(sys.argv[2], sys.argv[3], findings)
    else:
        scan_staged(findings)
    if not findings:
        return 0
    action = {"push": "Push", "range": "Check"}.get(mode, "Commit")
    print(f"\n{action} blocked: possible personal/financial data or secrets (see AGENTS.md).\n", file=sys.stderr)
    for finding in findings:
        print(f"  {finding}", file=sys.stderr)
    print("\nRemove or replace the data with synthetic values. For a known-safe line, add the marker", file=sys.stderr)
    print(f"`{ALLOW_MARKER}` on that line (with the project owner's approval). Do not bypass with --no-verify.\n",
          file=sys.stderr)
    return 1


if __name__ == "__main__":
    sys.exit(main())

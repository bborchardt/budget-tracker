# AGENTS.md

Guidance for AI coding agents (and humans) working on this repository.

## Project

A personal, local-only desktop app for planning and tracking a household's year: projected income, savings, and annual spending budgets by category, tracked weekly. A key feature is turning messy pasted/CSV bank data into clean, tagged transactions.

- **Source of truth:** `requirements.md`. Read it before making changes, and keep it up to date when decisions change.
- **Stack:** Tauri (Rust backend) + Svelte + TypeScript frontend, SQLite via `rusqlite`, custom CSS, Rust built-in tests + Vitest.

## Data Safety & Git — CRITICAL

This app handles personal financial data. **Nothing personal or financial may ever be committed or pushed.** A secret or a transaction that reaches a pushed commit should be considered permanently exposed, even if it is removed later.

### Never commit

- **Real financial data**: transactions, bank/card CSV exports, text pasted from bank websites, balances, income amounts, budgets, merchant histories
- **Account identifiers**: account numbers, card numbers (including last 4 digits), routing numbers, statement IDs
- **Database files**: `*.db`, `*.sqlite`, `*.sqlite3`, `*-wal`, `*-shm`, `*-journal`, and backup/export files from the app
- **Anything from the app's data directory** (`~/Library/Application Support/budget-tracker`)
- **Secrets**: API keys, tokens, passwords, `.env` files, local model server credentials
- **Personal identifiers**: real names, addresses, emails, phone numbers, employer names
- **Local tool settings**: `.claude/settings.local.json` and similar per-machine config

### Test data must be synthetic

- Build all fixtures from scratch with obviously fake values: merchants like "Example Grocery" or "Test Coffee Co", round or made-up amounts, fake account labels like "Card A".
- When reproducing a real bank's paste or CSV **format**, recreate the structure (column headers, date styles, layout quirks) by hand. **Never sanitize a real export or paste** — sanitizing reliably misses something.
- Synthetic fixtures live only under `tests/fixtures/` (the only place CSV files are allowed by `.gitignore`).

### Working with real data

- Real data used for debugging stays **outside the repository** (e.g., a folder in your home directory, not inside this repo).
- Never copy real data into code, comments, tests, commit messages, PR descriptions, issues, or docs.
- Never include real financial details the user mentions in conversation (income, balances, account names, merchants) in any committed file. Use generic examples instead.

### Git hooks (enforced checks)

`.githooks/` contains a **pre-commit** hook (scans staged changes) and a **pre-push** hook (re-scans every commit being pushed). Both run `.githooks/check-sensitive.py`, which blocks:

- Database files (by extension **and** by SQLite file contents), exports/backups, `.env`/key files, local tool settings
- CSV/TSV files outside `tests/fixtures/`
- Lines that look like card numbers (Luhn-checked), masked card numbers, labeled account/routing numbers, SSNs, non-`example.com` email addresses, or API keys/secrets

Setup (once per clone): `git config core.hooksPath .githooks`

Rules:

- **Never bypass the hooks** (`--no-verify`, unsetting `core.hooksPath`, editing the check to make a commit pass).
- If a line is a false positive, the `pii-allow` marker on that line skips it — **only with the user's approval**, and preferably by changing the fake value instead.
- The hooks are a safety net, not a substitute for reviewing the diff.

### Branch protection & CI

- `main` is protected by a GitHub ruleset: no direct pushes, no force-pushes, no deletion. **All changes go through pull requests.**
- The **`sensitive-data`** workflow (`.github/workflows/sensitive-data.yml`) runs the same check on every commit in a pull request and is a **required status check**. It catches anything the local hooks missed.
- Work on a branch, then open a pull request. Never try to work around the ruleset.

### Before every commit

Agents must:

1. **Commit only when the user asks. Never push unless the user explicitly asks. Never force-push.**
2. Run `git status` and review the **full** `git diff --staged`, not just file names.
3. **Stage files by name.** Never use `git add -A`, `git add .`, or `git commit -a`.
4. Confirm no staged file matches the "Never commit" list, and no new file type has slipped past `.gitignore`.
5. If anything looks like real data or a secret, **stop and ask the user** before continuing.

### If something sensitive is committed

Stop and tell the user immediately. Do **not** try to fix it with a follow-up commit — the data stays in history. It needs a history rewrite, and if it was already pushed, the data (or secret) must be treated as exposed (e.g., rotate keys).

### In application code

- Do not log transaction data, descriptions, amounts, or balances to console or log files. Error messages should describe the problem without including the data.
- No telemetry or analytics. The only permitted network traffic is to a user-configured **localhost** model server for optional AI assist (see `requirements.md`).
- Keep `.gitignore` current whenever a new kind of data file is introduced (e.g., a new backup or export format).

## How We Work

- Requirements first: discuss and record decisions in `requirements.md`, then implement.
- Keep scope focused; avoid over-engineering.
- Prefer a clear recommendation over a list of options.
- Work in small, reviewable steps and summarize what changed.

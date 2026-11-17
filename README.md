# budget-tracker

Local-only desktop app for planning and tracking a household's year: projected income, savings, and annual spending budgets by category, tracked weekly. Turns messy pasted or CSV bank data into clean, tagged transactions.

Built with Tauri (Rust + SQLite) and Svelte. All data stays on this machine.

- Requirements: [requirements.md](requirements.md)
- Development and data-safety rules: [AGENTS.md](AGENTS.md)

```sh
npm install
git config core.hooksPath .githooks
BUDGET_TRACKER_DATA_DIR=/tmp/budget-dev npm run tauri dev
```

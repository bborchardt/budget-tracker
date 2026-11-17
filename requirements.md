# Budget Tracker — Requirements

## Overview

A personal desktop application for planning and tracking a household's year: projected income, money held back for (or drawn from) savings, and annual spending budgets by category, with progress tracked weekly. A key feature is taking messy transaction data (CSV exports, copy-pasted text) and helping clean and tag it quickly. Data is stored locally. The app is built with Tauri (Rust backend + web frontend).

## Goals

- Build an annual plan: projected income − savings hold-back + savings draws = money budgeted across categories
- Track actual income against projections, by source
- Track spending against annual category budgets, week by week
- Track liquid savings balances and planned contributions/draws
- Adjust any part of the plan mid-year, with a full history of changes
- Turn messy input (CSV files, pasted text) into clean, tagged transactions with as little manual work as possible
- Keep all data local — no cloud dependency
- Export data for record-keeping or analysis
- Simple, clean interface focused on tables and summaries

## Non-Goals (v1)

- No cloud sync or multi-device support
- No automatic bank feeds (Plaid, Yodlee, etc.)
- No savings goals (e.g., "save $5,000 for a car by June"); savings is tracked only as part of the annual plan
- No recurring transaction templates
- No receipt/image attachments
- No tax-deductible flags or merchant tracking
- No multi-user or shared accounts
- No tracking of checking or credit card accounts (only savings accounts in v1)
- No retirement or brokerage account tracking
- No gross-income or tax tracking (income is take-home only)

## Core Concepts

### Annual Plan (Budget)

One plan per calendar year (Jan 1 — Dec 31) covering the whole household.

```
  Projected income (sum of income sources)
− Planned hold-back to savings
+ Planned draws from savings
= Money available to budget
  → fully assigned across spending categories
```

- **Year**: The calendar year the plan applies to
- **Income sources**: Projected take-home income by source
- **Savings plan**: Planned hold-back to savings and planned draws from savings for the year
- **Categories**: Spending categories with annual limits
- **Balance**: Every dollar available must be assigned to a category. While the plan is being built or changed, the app shows any unassigned amount (e.g., "$1,200 not yet assigned").
- **Status**: Active (current year), archived (past years, still viewable), or future (created in advance)
- A new year's plan can be created by duplicating the previous one (sources, categories, limits)

### Income Source

A source of take-home income with an annual projection.

- Examples: "Salary" ($78,000/yr), "Bonus" ($8,000/yr), "Side work" ($4,000/yr), "Other / Gifts" ($0/yr)
- Amounts are **take-home** (what lands in the bank), not gross
- **Timing**: Steady (spread evenly across the year) or lumpy, with expected month(s) (e.g., a bonus expected in a particular month)
- On track = year-to-date actual vs. expected by this point in the year, based on timing
- Unplanned income (e.g., a birthday check) goes to a source with a $0 projection such as "Other / Gifts"
- **Other / Gifts** is built in and is the default for income the app can't match to a source (marked "auto," like Misc)
- Projections can be adjusted mid-year (logged like category adjustments)
- **Estimated vs. confirmed**: A projection is marked as an estimate until the actual amount is known (e.g., a bonus or other irregular income estimated on Jan 1). When actuals arrive, the app shows actual vs. estimate and the resulting plan change.

### Split-Year Income (e.g., annual bonus)

An income source whose payment funds part of the current year and part of the next (e.g., an annual bonus).

- The payment funds the **12 months from its arrival**. Example: arriving at the start of May → 8/12 to the current year, 4/12 to the next year
- **Default split**: months from the expected arrival month through December ÷ 12 (editable)
- **Current-year share** counts as income in this year's plan
- **Next-year share** sits in savings: it is an automatic savings hold-back in this year's plan and an automatic savings draw ("2026 bonus carried in") in next year's plan
- On Jan 1 the plan includes the **known** carried-in share of last year's bonus and the **estimated** current-year share of this year's bonus
- When the actual bonus arrives, the app shows actual vs. estimate for both shares. The current-year difference leaves the plan unbalanced until the user adjusts it (logged as an adjustment); the next-year carry-forward updates automatically.

### Savings Account

A liquid savings account (e.g., savings, high-yield savings, money market). Expected: a few accounts.

- **Name**: e.g., "Savings 1"
- **Starting balance**: Balance at the start of tracking
- **Calculated balance**: Starting balance + transfers in − transfers out
- **Balance snapshots**: Occasional manually entered balances (e.g., "$24,310 as of Nov 1"). A snapshot corrects drift from missed transfers and captures interest. Interest is not tracked as income.
- **Plan tracking**: Actual transfers to/from savings year-to-date vs. the plan's hold-back and draws (e.g., "$7,500 of $10,000 saved; expected $8,600 by now")

### Category

A spending category with a name and an annual spending limit.

- Examples: "Groceries" ($6,000/yr), "Dining" ($3,000/yr), "Vacation Fund" ($2,500/yr), "Misc" ($1,000/yr)
- Spending limit is annual (not weekly)
- Can be added, removed, or have its limit changed mid-year
- **Removing a category = merging it** into another category (default: Misc). Its transactions move to the target, and its limit optionally moves too as a reallocation (on by default). Logged as one adjustment (e.g., "Removed Coffee → merged into Dining (40 transactions, $600 limit)"). Learned tagging updates to the target category.
- A category with no transactions is simply deleted (with the option to move its limit)
- Categories belong to a single year's plan; removing one does not affect past years
- **Misc** is a built-in category present in every plan (cannot be deleted). It holds spending that doesn't fit elsewhere, and it is the default for any expense the app can't confidently categorize. There is no separate "Uncategorized" bucket: tagging should never become a chore.
- Expenses the app *guessed* into Misc carry a quiet **"auto" marker** (no badge or nag). The user can filter to them and re-tag if they want; re-tagging teaches the app for next time.

### Plan Adjustment

A change to any part of the plan after it was set: a category limit, an income projection, or the savings hold-back/draw.

- **Old value** → **New value** (with delta)
- **Date of change**
- **Reason** (optional free-text note)
- Full history of adjustments is preserved and visible
- **Reallocation**: A single adjustment that moves an amount from one category to another (e.g., "Moved $1,000 Transport → Dining"). The total is unchanged, and the change is logged as one linked entry, not two unrelated ones.
- After an adjustment, the plan must balance again; the app shows any unassigned or over-assigned amount until it does

### Weeks & Pace

- **Weeks run Friday through Thursday.** Imports typically happen a couple of days after the week ends, which gives card transactions time to post, so the week that just ended is close to complete at import time.
- Transactions are assigned to weeks by **transaction date**, not posting date. A late-posting transaction lands in the correct week when it is imported later.
- **Partial weeks at year edges**: Week 1 runs from Jan 1 through the first Thursday; the last week ends Dec 31. Every week belongs to exactly one plan year.
- Partial weeks are labeled as partial in weekly comparisons
- **Pace-based colors** (per category):
  - 🟢 Green: at or under pace
  - 🟡 Yellow: ahead of pace by up to the warning threshold (default 10%, one app-wide setting)
  - 🔴 Red: ahead of pace by more than the threshold, or over the annual limit (shown as "$X over")
- **After an adjustment**, pace uses the current limit for the whole year: expected by now = new limit × fraction of year elapsed. Weekly and history views mark when the limit changed (e.g., "↑ $3,600 on Jul 1"); the adjustment log keeps old/new values and the reason.
- **Remaining weekly target** = (current limit − spent) ÷ weeks remaining
- **Lumpy categories** (e.g., Vacation Fund, holiday gifts, annual insurance): flagged per category, with no pacing. Yellow at 90% of the limit used; red when over the limit. Never colored for timing alone.
- **Pace is day-based**: "expected by now" = annual amount × days elapsed ÷ days in year (365/366). Weeks are only used to group spending for display, so short weeks and 53-week years don't distort pace.

### Transaction

A single financial event.

- **Date**: When the transaction occurred
- **Description**: Human-readable label (e.g., "EXAMPLE GROCERY #1234")
- **Amount**: Signed amount
- **Type**:
  - **Expense**: Tagged to a category. Includes refunds.
  - **Income**: Tagged to an income source
  - **Transfer**: Money moving between the user's own accounts (e.g., credit card payments, checking → savings). Never counts as spending or income. Tagged to a savings account when one is involved, so savings tracking stays accurate. The two sides of a transfer (e.g., card payment on the card statement and on the checking statement) are not paired in v1; each is simply marked Transfer.
- **Refunds**: Recorded as an Expense with a positive amount in the same category as the original purchase. Category spending = purchases minus refunds. Refunds are never income.
- **Notes** (optional): Free-form text

## Functional Requirements

### Annual Plan

- [ ] Create a plan for a year; duplicate a prior year's plan as a starting point
- [ ] Add, edit, remove income sources with annual projections and timing
- [ ] Set planned savings hold-back and draws
- [ ] Add, edit, remove categories with annual limits (removal merges transactions and optionally the limit into another category)
- [ ] Show unassigned / over-assigned amount until the plan balances
- [ ] Mark plans as active/archived

### Plan Adjustment

- [ ] Change a category limit, income projection, or savings hold-back/draw mid-year
- [ ] Reallocate an amount from one category to another in a single action
- [ ] Record a reason for the adjustment (optional)
- [ ] View full adjustment history
- [ ] See impact of adjustments on weekly progress: pace recalculates against the new limit, with a marker showing when the change happened

### Transaction Management

- [ ] Create, edit, delete transactions (expense, income, transfer)
- [ ] Filter transactions by date range, category, income source, keyword, type
- [ ] Sort by date, amount, category, type
- [ ] Bulk operations (delete multiple, edit multiple)

### Income Tracking

- [ ] Show year-to-date actual vs. projected income, overall and by source
- [ ] Show expected-by-now for each source based on its timing (steady or lumpy)
- [ ] Mark projections as estimated or confirmed; show actual vs. estimate when actuals arrive
- [ ] Split-year income: set the current/next-year split, carry the next-year share forward automatically as a savings hold-back and next-year draw

### Savings Tracking

- [ ] Add, edit, remove savings accounts with starting balance
- [ ] Record balance snapshots
- [ ] Show calculated balance, latest snapshot, and the difference
- [ ] Show actual vs. planned hold-back and draws, year-to-date and expected-by-now

### Weekly Spending Progress

- [ ] Show current year's spending by category vs. annual budget
- [ ] Show percentage of annual budget used (e.g., "40% of $6,000 spent")
- [ ] Show remaining amount and average weekly spend needed to stay on track
- [ ] Visual indicator based on pace: on track (green), ahead of pace (yellow), well ahead of pace or over budget (red); lumpy categories use % of limit only
- [ ] Mark a category as lumpy (no pacing)
- [ ] App setting for the pace warning threshold (default 10%)
- [ ] Over-budget categories display clearly (e.g., "$300 over") with negative remaining, rather than capping at 100%
- [ ] Weekly breakdown: how much was spent in the current week vs. prior weeks
- [ ] Rolling view: compare current week to prior weeks

### Import & Cleanup (key feature)

Taking messy input and helping clean and tag it is central to the app's value.

**Sources & inputs**
- Expected sources: a handful of credit card and checking accounts, imported weekly
- [ ] Import from CSV files (bank/credit card exports)
- [ ] Import from copy-pasted text (primary method: copied from bank/card websites). Support pasting separate pending and posted blocks together or one after another.
- [ ] Each source has a named, remembered format (e.g., "Card A"); after first-time column confirmation, later imports from that source skip mapping
- [ ] Each transaction records which source it came from (a label only, not full account tracking)
- [ ] Nice-to-have: detect the source automatically from the pasted format

**Parsing & cleanup**
- [ ] Dates: common formats (`03/14/2026`, `2026-03-14`, `Mar 14`, `14 Mar 2026`); infer a missing year from context
- [ ] Amounts: signed, parenthesized, `$`-prefixed, `CR`/`DR` suffixes, separate debit/credit columns; detect per source whether purchases are shown positive and normalize the sign
- [ ] Drop junk rows: headers, totals, balance lines, blanks, repeated page headers
- [ ] Keep the original description and show a cleaned display name (e.g., "SQ *EXAMPLE COFFEE 0423 ANYTOWN ST" → "Example Coffee")
- [ ] Column mapping when a format is new or unrecognized

**Pending transactions**
- [ ] Import pending transactions, tagged and counted, with a "pending" marker (they matter for seeing where things stand at a point in time)
- [ ] On later imports, automatically replace a pending item with its posted version when matched: same source, date within ~5 days, similar description, amount within a tolerance (handles tips and gas-station holds). Keep the user's tag.
- [ ] Automatically remove pending items not matched after 10 days (canceled orders, holds), with undo
- [ ] Import summary reports pending changes (e.g., "6 pending → posted, 1 amount changed; 2 stale pending removed")

**Tagging**
- [ ] Suggest type and category/source/account for each row; user confirms or corrects
- [ ] Unrecognized expenses default to Misc and unrecognized income to Other / Gifts, marked "auto"; filter to auto-tagged rows to optionally re-tag
- [ ] Learn from re-tagging so the same merchant/description is categorized correctly next time
- [ ] Recognize common transfer patterns (e.g., "PAYMENT THANK YOU," "AUTOPAY") and learn the user's own, suggesting Transfer

**Suggestions: local learning + optional local AI**
- [ ] **Local learning (always on, runs first)**: tag from the user's own history (merchant → category/source/Transfer) plus built-in obvious patterns (e.g., AUTOPAY → Transfer, payroll → Salary). Parse using each source's remembered format.
- [ ] **Optional AI assist (off by default)**: used only for rows local learning can't handle: unrecognized/changed paste layouts and first-time merchants
- [ ] AI uses a **local model server only** (e.g., an OpenAI-compatible endpoint on localhost), configured in settings. **No cloud AI option.** Transaction data never leaves the machine.
- [ ] AI suggestions are marked "auto," like Misc defaults; user corrections feed local learning, so AI is needed less over time
- [ ] The app works fully without AI configured or when the local model server is not running

**Review & commit (weekly import workflow — minimal clicks)**
- [ ] One Import screen (keyboard shortcut); paste or drop multiple sources in one session, with source detected or chosen per block
- [ ] One review table of all incoming rows, sorted so items needing attention (auto tags, possible duplicates) are at the top; confident rows arrive already tagged; pending rows marked
- [ ] Keyboard-driven editing: arrow between rows, type-ahead category picker, multi-select bulk tagging
- [ ] Fixing a tag teaches the app for that merchant (no separate rule setup)
- [ ] No forced review: no per-row approval, no required fields, auto tags can be left as-is
- [ ] Single commit action ("Import 47 transactions") with a summary: new, duplicates skipped, pending → posted, stale pending removed
- [ ] Undo an entire import as one batch
- [ ] After commit, land on the dashboard showing where things stand after the completed week
- [ ] **Deduplication** (critical for weekly copy-paste with overlapping date ranges):
  - Match on **source + date + amount**, with a **similar** description (descriptions vary between CSV and website copy)
  - **Count-aware**: if the database has one identical purchase and the input has two, import one. Identical purchases are never collapsed.
  - Clear duplicates are skipped automatically and reported in the summary (e.g., "38 already imported, skipped")
  - Only uncertain matches (e.g., date off by a day, different-looking description) are shown for a quick "Same as this one? Yes / No," with the existing row alongside
  - Pending → posted is handled by pending replacement, not treated as a duplicate

### CSV Export

- [ ] Export transactions to CSV
- [ ] Export plan and budget summaries to CSV
- [ ] Export selectable date range

### Reporting & Summaries

- [ ] **Dashboard**: income vs. projection, savings vs. plan, total budget / spent / remaining across all categories
- [ ] **Category progress**: spending progress per category with budget vs. actual, remaining, percentage used
- [ ] **Weekly breakdown**: how much was spent in the current week vs. prior weeks
- [ ] **Adjustment log**: history of all plan changes with dates and reasons
- [ ] **Historical view**: browse past years' plans and actuals
- [ ] **Category trend**: how spending in a category has changed over the last 12 weeks

### Data Management

- [ ] Local SQLite database for all data
- [ ] Export entire database as a backup file
- [ ] Import database from a backup file
- [ ] Clear all data (with confirmation)

## Non-Functional Requirements

### Performance

- App should launch in < 2 seconds on a typical machine
- Loading the main view with 10,000 transactions should take < 500ms
- Database queries should be indexed and efficient

### Data Storage

- SQLite database stored locally in the app's data directory
- Data directory location: `~/Library/Application Support/budget-tracker` (macOS)
- No data leaves the machine — no telemetry, no analytics, no cloud

### Security

- No network requests by default
- The only network traffic permitted is to a user-configured **local** model server (localhost) for optional AI assist; no external/cloud endpoints
- Import is user-initiated only (no auto-sync)
- No user authentication (single-user app)

### UX

- Clean, minimal design — no clutter
- Desktop-native feel (uses system fonts, appropriate spacing)
- Responsive UI — no freezing during data operations
- Keyboard shortcuts for common actions (new transaction, import, etc.)

## Tech Stack

| Layer | Technology |
|-------|-----------|
| Framework | Tauri (Rust backend + web frontend) |
| Backend language | Rust |
| Database | SQLite via `rusqlite` |
| Frontend framework | Svelte |
| Frontend language | TypeScript |
| Styling | Custom CSS (system fonts, no framework) |
| Testing | Rust built-in tests (backend) + Vitest (frontend) |
| Packaging | Unsigned DMG (personal use) |

## Technical Decisions (decided 2026-11-14)

1. **Frontend framework: Svelte.** Lighter than React, fits Tauri well, and less to learn alongside Rust.
2. **Database: `rusqlite`.** Simple and synchronous; plenty for a single-user local app.
3. **Styling: Custom CSS.** The UI is mostly tables and should feel minimal and native.
4. **Testing: Rust built-in tests + Vitest.** Covers the backend logic and the frontend.
5. **Packaging: Unsigned DMG.** Personal use only; signing can come later if needed.

## Implementation Decisions (2026-11-15, easy to revisit)

Made during the first build without blocking on a question; each is isolated and easy to change.

1. **Money** is stored as integer cents; dates as ISO text. Transaction amounts are signed from the household's view (negative = money out).
2. **Adjustment logging starts when a plan has its first transaction.** Before that, edits are setup and aren't logged.
3. **Learned tagging rules are keyed by merchant** (cleaned description without store numbers/locations) and store category/source **names**, so they carry across years.
4. **Numeric dates are month-first** (US). A date without a year is the most recent such date.
5. **Sign convention per source** is guessed on first import (most amounts positive → purchases shown positive) and remembered; a "Flip signs" control corrects it.
6. **Possible duplicates default to "import"** (not a duplicate) until the user says "Yes, same."
7. **Pending → posted matching:** same source, posted 1 day before to 5 days after, similar merchant, amount within 30% (min $2) or a pending hold of $1 or less.
8. **Deleted transactions are kept hidden** so a later overlapping import doesn't bring them back.
9. **Pace colors:** yellow as soon as spending is ahead of an even pace; red at 10 points of the annual budget ahead (setting). Lumpy: yellow at 90% used.
10. **Income "expected by now":** steady sources accrue by day; lumpy sources count each expected month once it has ended.
11. **Savings "expected by now":** the manual hold-back accrues by day; split-year shares are expected after the payment's month.
12. **Undo** is allowed for any import unless a later import already replaced one of its pending rows.
13. **Debit/credit columns without headers** are assumed to be debit first.
14. **Dev data:** `BUDGET_TRACKER_DATA_DIR` points the app at a throwaway database.

### Not built yet

- AI assist (settings exist; only localhost endpoints accepted)
- CSV export (transactions, summaries) and database backup/restore
- Manual column mapping for formats the parser can't read automatically
- Category trend view (backend command exists, no screen yet)
- Arrow-key row navigation in the import review (tab + type-to-select works)

## Assumptions

- The user is the only user of the app (single-user, no auth)
- One plan per calendar year (Jan 1 — Dec 31) for the whole household
- Income is planned and tracked as take-home pay
- Weekly tracking shows progress against the annual budget
- The user has bank/credit card CSV exports or copyable transaction lists available for import
- The user is comfortable learning basic Rust

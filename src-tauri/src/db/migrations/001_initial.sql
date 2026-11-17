-- Initial schema. See requirements.md for the concepts.
--
-- Conventions:
--   * Money is stored as integer cents (INTEGER), never floating point.
--   * Dates are ISO-8601 text: 'YYYY-MM-DD'; timestamps are 'YYYY-MM-DD HH:MM:SS' (UTC).
--   * Transaction amounts are signed from the household's point of view:
--     negative = money out (purchases, transfers out of checking), positive = money in
--     (refunds, income, transfers in). The importer normalizes each source to this.
--   * Rows referenced by the history (categories, income sources) are soft-removed with
--     removed_at so the adjustment log keeps pointing at something.

-- One plan per calendar year for the whole household.
CREATE TABLE plans (
    id                     INTEGER PRIMARY KEY,
    year                   INTEGER NOT NULL UNIQUE CHECK (year BETWEEN 2000 AND 2200),
    -- Manually planned savings for the year (split-year income adds automatic amounts on top)
    planned_holdback_cents INTEGER NOT NULL DEFAULT 0 CHECK (planned_holdback_cents >= 0),
    planned_draw_cents     INTEGER NOT NULL DEFAULT 0 CHECK (planned_draw_cents >= 0),
    archived               INTEGER NOT NULL DEFAULT 0 CHECK (archived IN (0, 1)),
    created_at             TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE categories (
    id                 INTEGER PRIMARY KEY,
    plan_id            INTEGER NOT NULL REFERENCES plans(id) ON DELETE CASCADE,
    name               TEXT NOT NULL CHECK (length(trim(name)) > 0),
    annual_limit_cents INTEGER NOT NULL DEFAULT 0 CHECK (annual_limit_cents >= 0),
    is_lumpy           INTEGER NOT NULL DEFAULT 0 CHECK (is_lumpy IN (0, 1)),
    -- Built-in Misc: one per plan, cannot be removed, default for unrecognized expenses
    is_misc            INTEGER NOT NULL DEFAULT 0 CHECK (is_misc IN (0, 1)),
    sort_order         INTEGER NOT NULL DEFAULT 0,
    removed_at         TEXT,
    merged_into_id     INTEGER REFERENCES categories(id)
);
CREATE UNIQUE INDEX categories_active_name ON categories(plan_id, name COLLATE NOCASE)
    WHERE removed_at IS NULL;
CREATE UNIQUE INDEX categories_one_misc ON categories(plan_id) WHERE is_misc = 1;

CREATE TABLE income_sources (
    id              INTEGER PRIMARY KEY,
    plan_id         INTEGER NOT NULL REFERENCES plans(id) ON DELETE CASCADE,
    name            TEXT NOT NULL CHECK (length(trim(name)) > 0),
    -- Take-home projection for the year (for split-year sources: the full payment)
    projected_cents INTEGER NOT NULL DEFAULT 0 CHECK (projected_cents >= 0),
    timing          TEXT NOT NULL DEFAULT 'steady' CHECK (timing IN ('steady', 'lumpy')),
    -- Comma-separated months (1-12) a lumpy source is expected in, e.g. '3' or '6,12'
    expected_months TEXT NOT NULL DEFAULT '',
    is_estimate     INTEGER NOT NULL DEFAULT 0 CHECK (is_estimate IN (0, 1)),
    -- Built-in Other / Gifts: one per plan, default for unrecognized income
    is_other        INTEGER NOT NULL DEFAULT 0 CHECK (is_other IN (0, 1)),
    -- Split-year income: twelfths that fund the current year (e.g. 8 = 8/12 this year,
    -- 4/12 carried to next year through savings). NULL = not split.
    split_current_twelfths INTEGER CHECK (split_current_twelfths BETWEEN 0 AND 12),
    sort_order      INTEGER NOT NULL DEFAULT 0,
    removed_at      TEXT
);
CREATE UNIQUE INDEX income_sources_active_name ON income_sources(plan_id, name COLLATE NOCASE)
    WHERE removed_at IS NULL;
CREATE UNIQUE INDEX income_sources_one_other ON income_sources(plan_id) WHERE is_other = 1;

-- Liquid savings accounts (not per plan: balances run across years)
CREATE TABLE savings_accounts (
    id                     INTEGER PRIMARY KEY,
    name                   TEXT NOT NULL UNIQUE COLLATE NOCASE CHECK (length(trim(name)) > 0),
    starting_balance_cents INTEGER NOT NULL DEFAULT 0,
    starting_date          TEXT NOT NULL,
    archived               INTEGER NOT NULL DEFAULT 0 CHECK (archived IN (0, 1)),
    created_at             TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE balance_snapshots (
    id            INTEGER PRIMARY KEY,
    account_id    INTEGER NOT NULL REFERENCES savings_accounts(id) ON DELETE CASCADE,
    date          TEXT NOT NULL,
    balance_cents INTEGER NOT NULL,
    note          TEXT NOT NULL DEFAULT '',
    created_at    TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX balance_snapshots_account_date ON balance_snapshots(account_id, date);

-- Every change to a plan after its year has started
CREATE TABLE adjustments (
    id               INTEGER PRIMARY KEY,
    plan_id          INTEGER NOT NULL REFERENCES plans(id) ON DELETE CASCADE,
    date             TEXT NOT NULL,
    kind             TEXT NOT NULL CHECK (kind IN (
                         'category_limit',     -- category_id, old/new
                         'category_added',     -- category_id, new
                         'category_removed',   -- category_id merged into to_category_id
                         'reallocation',       -- category_id -> to_category_id, amount
                         'income_projection',  -- income_source_id, old/new
                         'income_added',       -- income_source_id, new
                         'income_removed',     -- income_source_id, old
                         'savings_holdback',   -- old/new
                         'savings_draw'        -- old/new
                     )),
    category_id      INTEGER REFERENCES categories(id),
    to_category_id   INTEGER REFERENCES categories(id),
    income_source_id INTEGER REFERENCES income_sources(id),
    old_cents        INTEGER,
    new_cents        INTEGER,
    amount_cents     INTEGER,
    -- e.g. number of transactions moved by a category merge
    detail           TEXT NOT NULL DEFAULT '',
    reason           TEXT NOT NULL DEFAULT '',
    created_at       TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX adjustments_plan_date ON adjustments(plan_id, date);

-- Where transactions come from (a card or checking account); a label plus its remembered format
CREATE TABLE import_sources (
    id          INTEGER PRIMARY KEY,
    name        TEXT NOT NULL UNIQUE COLLATE NOCASE CHECK (length(trim(name)) > 0),
    kind        TEXT NOT NULL CHECK (kind IN ('card', 'checking')),
    -- JSON: remembered parsing choices, e.g. {"purchases_positive": true}
    format_json TEXT NOT NULL DEFAULT '{}',
    created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

-- One committed import (possibly several sources); undoable as a unit
CREATE TABLE import_batches (
    id           INTEGER PRIMARY KEY,
    created_at   TEXT NOT NULL DEFAULT (datetime('now')),
    summary_json TEXT NOT NULL DEFAULT '{}',
    undone_at    TEXT
);

CREATE TABLE transactions (
    id                  INTEGER PRIMARY KEY,
    date                TEXT NOT NULL,
    -- Original text from the bank, kept for matching; display_name is the cleaned version
    description         TEXT NOT NULL,
    display_name        TEXT NOT NULL,
    merchant_key        TEXT NOT NULL DEFAULT '',
    amount_cents        INTEGER NOT NULL,
    type                TEXT NOT NULL CHECK (type IN ('expense', 'income', 'transfer')),
    category_id         INTEGER REFERENCES categories(id),
    income_source_id    INTEGER REFERENCES income_sources(id),
    savings_account_id  INTEGER REFERENCES savings_accounts(id),
    source_id           INTEGER REFERENCES import_sources(id),
    is_pending          INTEGER NOT NULL DEFAULT 0 CHECK (is_pending IN (0, 1)),
    -- Tag was guessed (fallback to Misc / Other, or AI) rather than learned or chosen
    is_auto             INTEGER NOT NULL DEFAULT 0 CHECK (is_auto IN (0, 1)),
    notes               TEXT NOT NULL DEFAULT '',
    import_batch_id     INTEGER REFERENCES import_batches(id),
    -- Soft delete, used by pending replacement and stale-pending removal so undo can restore
    deleted_at          TEXT,
    deleted_by_batch_id INTEGER REFERENCES import_batches(id),
    replaced_by_id      INTEGER REFERENCES transactions(id),
    created_at          TEXT NOT NULL DEFAULT (datetime('now')),
    CHECK (type <> 'expense'  OR (category_id IS NOT NULL AND income_source_id IS NULL)),
    CHECK (type <> 'income'   OR (income_source_id IS NOT NULL AND category_id IS NULL)),
    CHECK (type <> 'transfer' OR (category_id IS NULL AND income_source_id IS NULL))
);
CREATE INDEX transactions_date ON transactions(date) WHERE deleted_at IS NULL;
CREATE INDEX transactions_category ON transactions(category_id) WHERE deleted_at IS NULL;
CREATE INDEX transactions_income_source ON transactions(income_source_id) WHERE deleted_at IS NULL;
CREATE INDEX transactions_savings ON transactions(savings_account_id) WHERE deleted_at IS NULL;
CREATE INDEX transactions_source_date ON transactions(source_id, date) WHERE deleted_at IS NULL;
CREATE INDEX transactions_batch ON transactions(import_batch_id);
CREATE INDEX transactions_deleted_batch ON transactions(deleted_by_batch_id);

-- Learned tagging: merchant -> tag. Category/source are stored by name so rules carry
-- across years (each year's plan has its own category rows).
CREATE TABLE tag_rules (
    id                  INTEGER PRIMARY KEY,
    merchant_key        TEXT NOT NULL UNIQUE,
    type                TEXT NOT NULL CHECK (type IN ('expense', 'income', 'transfer')),
    category_name       TEXT,
    income_source_name  TEXT,
    savings_account_id  INTEGER REFERENCES savings_accounts(id) ON DELETE SET NULL,
    times_used          INTEGER NOT NULL DEFAULT 0,
    updated_at          TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
INSERT INTO settings (key, value) VALUES
    ('pace_warning_threshold_pct', '10'),
    ('lumpy_warning_pct', '90'),
    ('pending_stale_days', '10'),
    ('ai_enabled', 'false'),
    ('ai_endpoint', '');

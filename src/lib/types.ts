// Mirrors the Rust types returned by the Tauri commands (src-tauri/src).
// Money is always integer cents; dates are ISO strings (YYYY-MM-DD).

export type Status = 'green' | 'yellow' | 'red'
export type TxType = 'expense' | 'income' | 'transfer'

export interface Plan {
  id: number
  year: number
  planned_holdback_cents: number
  planned_draw_cents: number
  archived: boolean
}

export interface Category {
  id: number
  plan_id: number
  name: string
  annual_limit_cents: number
  is_lumpy: boolean
  is_misc: boolean
  sort_order: number
}

export interface IncomeSource {
  id: number
  plan_id: number
  name: string
  projected_cents: number
  timing: 'steady' | 'lumpy'
  expected_months: number[]
  is_estimate: boolean
  is_other: boolean
  split_current_twelfths: number | null
  sort_order: number
}

export interface IncomeSourceInput {
  name: string
  timing: 'steady' | 'lumpy'
  expected_months: number[]
  is_estimate: boolean
  split_current_twelfths: number | null
}

export interface PlanTotals {
  projected_income_cents: number
  auto_holdback_cents: number
  planned_holdback_cents: number
  total_holdback_cents: number
  carried_in_cents: number
  planned_draw_cents: number
  total_draw_cents: number
  available_cents: number
  assigned_cents: number
  unassigned_cents: number
}

export interface Progress {
  limit_cents: number
  spent_cents: number
  remaining_cents: number
  pct_used: number
  expected_cents: number | null
  ahead_cents: number | null
  over_cents: number
  weekly_target_cents: number | null
  status: Status
}

export interface CategoryRow extends Category {
  progress: Progress
  limit_changed_on: string | null
  auto_count: number
}

export interface IncomeRow extends IncomeSource {
  actual_cents: number
  expected_cents: number
  current_share_cents: number
  next_share_cents: number
  needs_confirmation: boolean
}

export interface Week {
  year: number
  index: number
  start: string
  end: string
  partial: boolean
}

export interface WeekSpend {
  week: Week
  spent_cents: number
}

export interface SavingsAccount {
  id: number
  name: string
  starting_balance_cents: number
  starting_date: string
  archived: boolean
}

export interface Snapshot {
  id: number
  account_id: number
  date: string
  balance_cents: number
  note: string
}

export interface AccountBalance {
  account: SavingsAccount
  calculated_cents: number
  latest_snapshot: Snapshot | null
  balance_cents: number
  drift_cents: number | null
}

export interface Overview {
  year: number
  today: string
  as_of: string
  fraction_elapsed: number
  plan: Plan | null
  tracking: boolean
  totals: PlanTotals | null
  categories: CategoryRow[]
  spending: { limit_cents: number; spent_cents: number; remaining_cents: number }
  income: IncomeRow[]
  income_totals: { projected_cents: number; actual_cents: number; expected_cents: number }
  savings: {
    planned_holdback_cents: number
    planned_draw_cents: number
    saved_cents: number
    drawn_cents: number
    expected_saved_cents: number
    expected_drawn_cents: number
    accounts: AccountBalance[]
    total_balance_cents: number
  }
  weeks: WeekSpend[]
  current_week: Week | null
  last_week: WeekSpend | null
  pending_count: number
  auto_count: number
}

export interface Adjustment {
  id: number
  date: string
  kind: string
  category_id: number | null
  category_name: string | null
  to_category_id: number | null
  to_category_name: string | null
  income_source_id: number | null
  income_source_name: string | null
  old_cents: number | null
  new_cents: number | null
  amount_cents: number | null
  detail: string
  reason: string
}

export interface Tag {
  type: TxType
  category_id?: number | null
  income_source_id?: number | null
  savings_account_id?: number | null
}

export interface Transaction {
  id: number
  date: string
  description: string
  display_name: string
  amount_cents: number
  type: TxType
  category_id: number | null
  category_name: string | null
  income_source_id: number | null
  income_source_name: string | null
  savings_account_id: number | null
  savings_account_name: string | null
  source_id: number | null
  source_name: string | null
  is_pending: boolean
  is_auto: boolean
  notes: string
  import_batch_id: number | null
}

export interface TransactionFilter {
  from?: string
  to?: string
  year?: number
  category_id?: number
  income_source_id?: number
  source_id?: number
  type?: TxType
  keyword?: string
  auto_only?: boolean
  pending_only?: boolean
  sort?: 'date' | 'amount' | 'category' | 'type'
  ascending?: boolean
  limit?: number
  offset?: number
}

export interface ImportSource {
  id: number
  name: string
  kind: 'card' | 'checking'
  format: { purchases_positive?: boolean | null }
}

export type MatchStatus =
  | { kind: 'new' }
  | { kind: 'duplicate'; existing_id: number }
  | { kind: 'replaces_pending'; existing_id: number; amount_changed: boolean }
  | { kind: 'possible_duplicate'; existing_id: number }

export interface Suggestion {
  tag: Tag
  auto: boolean
  reason: 'learned' | 'pattern' | 'fallback' | 'pending'
}

export interface PreviewRow {
  line: number
  date: string
  description: string
  display_name: string
  merchant_key: string
  amount_cents: number
  pending: boolean
  status: MatchStatus
  existing: { id: number; date: string; display_name: string; amount_cents: number; is_pending: boolean } | null
  suggestion: Suggestion | null
  problem: string | null
}

export interface Preview {
  source_id: number
  layout: string
  skipped_lines: number
  purchases_positive: boolean
  sign_detected: boolean
  rows: PreviewRow[]
}

export interface CommitRow {
  source_id: number
  date: string
  description: string
  amount_cents: number
  pending: boolean
  action: 'import' | 'skip' | 'replace_pending'
  existing_id: number | null
  tag: Tag | null
  auto: boolean
  learn: boolean
}

export interface ImportSummary {
  batch_id: number
  imported: number
  skipped: number
  pending_replaced: number
  pending_amount_changed: number
  stale_pending_removed: number
}

export interface Batch {
  id: number
  created_at: string
  summary: ImportSummary
  undone_at: string | null
}

export interface Settings {
  pace_warning_threshold_pct: number
  lumpy_warning_pct: number
  pending_stale_days: number
  ai_enabled: boolean
  ai_endpoint: string
}

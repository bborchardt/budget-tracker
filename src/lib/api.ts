// Typed wrappers around the Tauri commands in src-tauri/src/commands.rs.
import { invoke } from '@tauri-apps/api/core'
import type {
  AccountBalance,
  Adjustment,
  Batch,
  Category,
  CommitRow,
  ImportSource,
  ImportSummary,
  IncomeSource,
  IncomeSourceInput,
  Overview,
  Plan,
  Preview,
  SavingsAccount,
  Settings,
  Snapshot,
  Tag,
  Transaction,
  TransactionFilter,
  TxType,
  WeekSpend,
} from './types'

export const api = {
  today: () => invoke<string>('get_today'),
  overview: (year: number) => invoke<Overview>('get_overview', { year }),

  listPlans: () => invoke<Plan[]>('list_plans'),
  createPlan: (year: number, copyFrom: number | null) => invoke<Plan>('create_plan', { year, copyFrom }),
  setPlanArchived: (planId: number, archived: boolean) => invoke<Plan>('set_plan_archived', { planId, archived }),
  adjustments: (planId: number) => invoke<Adjustment[]>('list_adjustments', { planId }),

  addCategory: (planId: number, name: string, limitCents: number, lumpy: boolean) =>
    invoke<Category>('add_category', { planId, name, limitCents, lumpy }),
  updateCategory: (id: number, name: string, lumpy: boolean) => invoke<Category>('update_category', { id, name, lumpy }),
  setCategoryLimit: (id: number, limitCents: number, reason: string) =>
    invoke<Category>('set_category_limit', { id, limitCents, reason }),
  reallocate: (fromId: number, toId: number, amountCents: number, reason: string) =>
    invoke<void>('reallocate', { fromId, toId, amountCents, reason }),
  removeCategory: (id: number, mergeInto: number | null, moveLimit: boolean, reason: string) =>
    invoke<number>('remove_category', { id, mergeInto, moveLimit, reason }),
  categoryTrend: (categoryId: number) => invoke<WeekSpend[]>('category_trend', { categoryId }),

  addIncomeSource: (planId: number, input: IncomeSourceInput, projectedCents: number) =>
    invoke<IncomeSource>('add_income_source', { planId, input, projectedCents }),
  updateIncomeSource: (id: number, input: IncomeSourceInput) => invoke<IncomeSource>('update_income_source', { id, input }),
  setIncomeProjection: (id: number, projectedCents: number, confirm: boolean, reason: string) =>
    invoke<IncomeSource>('set_income_projection', { id, projectedCents, confirm, reason }),
  removeIncomeSource: (id: number, moveTo: number | null, reason: string) =>
    invoke<void>('remove_income_source', { id, moveTo, reason }),
  setSavingsPlan: (planId: number, holdbackCents: number, drawCents: number, reason: string) =>
    invoke<Plan>('set_savings_plan', { planId, holdbackCents, drawCents, reason }),

  listTransactions: (filter: TransactionFilter) =>
    invoke<{ rows: Transaction[]; total: number }>('list_transactions', { filter }),
  createTransaction: (t: { date: string; description: string; amount_cents: number; tag: Tag; notes: string }) =>
    invoke<Transaction>('create_transaction', { transaction: t }),
  updateTransaction: (
    id: number,
    update: { date: string; display_name: string; amount_cents: number; tag: Tag; notes: string; learn: boolean },
  ) => invoke<Transaction>('update_transaction', { id, update }),
  setTagBulk: (ids: number[], tag: Tag, learn: boolean) => invoke<number>('set_tag_bulk', { ids, tag, learn }),
  deleteTransactions: (ids: number[]) => invoke<number>('delete_transactions', { ids }),

  listSavings: () => invoke<AccountBalance[]>('list_savings'),
  addSavingsAccount: (name: string, startingBalanceCents: number, startingDate: string) =>
    invoke<SavingsAccount>('add_savings_account', { name, startingBalanceCents, startingDate }),
  updateSavingsAccount: (id: number, name: string, startingBalanceCents: number, startingDate: string, archived: boolean) =>
    invoke<SavingsAccount>('update_savings_account', { id, name, startingBalanceCents, startingDate, archived }),
  addSnapshot: (accountId: number, date: string, balanceCents: number, note: string) =>
    invoke<Snapshot>('add_snapshot', { accountId, date, balanceCents, note }),
  listSnapshots: (accountId: number) => invoke<Snapshot[]>('list_snapshots', { accountId }),
  deleteSnapshot: (id: number) => invoke<void>('delete_snapshot', { id }),

  listImportSources: () => invoke<ImportSource[]>('list_import_sources'),
  addImportSource: (name: string, kind: 'card' | 'checking') => invoke<ImportSource>('add_import_source', { name, kind }),
  updateImportSource: (id: number, name: string, kind: 'card' | 'checking') =>
    invoke<ImportSource>('update_import_source', { id, name, kind }),
  importPreview: (sourceId: number, text: string, purchasesPositive: boolean | null) =>
    invoke<Preview>('import_preview', { sourceId, text, purchasesPositive }),
  importCommit: (rows: CommitRow[], formats: { source_id: number; purchases_positive: boolean }[]) =>
    invoke<ImportSummary>('import_commit', { request: { rows, formats } }),
  undoImport: (batchId: number) => invoke<void>('undo_import', { batchId }),
  listImportBatches: () => invoke<Batch[]>('list_import_batches'),

  getSettings: () => invoke<Settings>('get_settings'),
  saveSettings: (settings: Settings) => invoke<Settings>('save_settings', { settings }),
}

/** Error text from a failed command (the backend sends plain strings). */
export function errorText(e: unknown): string {
  if (typeof e === 'string') return e
  if (e instanceof Error) return e.message
  return 'Something went wrong'
}

export type { TxType }

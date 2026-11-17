// Tag choices for selects: categories, income sources, and transfers in one list.
import type { CategoryRow, IncomeRow, Overview, SavingsAccount, Tag } from './types'

export interface TagOptions {
  categories: { id: number; name: string }[]
  income: { id: number; name: string }[]
  savings: { id: number; name: string }[]
}

export function optionsFrom(overview: Overview | null, savings: SavingsAccount[] = []): TagOptions {
  return {
    categories: (overview?.categories ?? []).map((c: CategoryRow) => ({ id: c.id, name: c.name })),
    income: (overview?.income ?? []).map((s: IncomeRow) => ({ id: s.id, name: s.name })),
    savings: savings.filter((a) => !a.archived).map((a) => ({ id: a.id, name: a.name })),
  }
}

/** Encode a tag as a select value: "e:12", "i:3", "t:", "t:4" */
export function tagKey(tag: Tag | null | undefined): string {
  if (!tag) return ''
  switch (tag.type) {
    case 'expense':
      return `e:${tag.category_id ?? ''}`
    case 'income':
      return `i:${tag.income_source_id ?? ''}`
    case 'transfer':
      return `t:${tag.savings_account_id ?? ''}`
  }
}

export function tagFromKey(key: string): Tag | null {
  const [kind, raw] = key.split(':')
  const id = raw ? Number(raw) : null
  switch (kind) {
    case 'e':
      return id === null ? null : { type: 'expense', category_id: id }
    case 'i':
      return id === null ? null : { type: 'income', income_source_id: id }
    case 't':
      return { type: 'transfer', savings_account_id: id }
    default:
      return null
  }
}

export function sameTag(a: Tag | null | undefined, b: Tag | null | undefined): boolean {
  return tagKey(a) === tagKey(b)
}

export function tagLabel(tag: Tag | null | undefined, opts: TagOptions): string {
  if (!tag) return '—'
  switch (tag.type) {
    case 'expense':
      return opts.categories.find((c) => c.id === tag.category_id)?.name ?? 'Category'
    case 'income':
      return `Income: ${opts.income.find((s) => s.id === tag.income_source_id)?.name ?? '?'}`
    case 'transfer': {
      const acct = opts.savings.find((a) => a.id === tag.savings_account_id)
      return acct ? `Transfer: ${acct.name}` : 'Transfer'
    }
  }
}

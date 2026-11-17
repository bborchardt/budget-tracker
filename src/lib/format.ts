// Formatting and parsing helpers. Money is integer cents throughout.

const usd = new Intl.NumberFormat('en-US', { style: 'currency', currency: 'USD' })
const usdWhole = new Intl.NumberFormat('en-US', { style: 'currency', currency: 'USD', maximumFractionDigits: 0 })

/** $1,234.56 (or −$1,234.56) */
export function money(cents: number | null | undefined, opts: { whole?: boolean; signed?: boolean } = {}): string {
  if (cents === null || cents === undefined) return '—'
  const fmt = opts.whole ? usdWhole : usd
  const text = fmt.format(Math.abs(cents) / 100)
  if (cents < 0) return `−${text}`
  if (opts.signed && cents > 0) return `+${text}`
  return text
}

/**
 * Parse what a person types as dollars into cents: "1,234.56", "$12", "-5", "(5.00)".
 * Returns null when it isn't a number.
 */
export function parseDollars(input: string): number | null {
  let s = input.trim().replace(/[\s,$]/g, '').replace(/−/g, '-')
  if (!s) return null
  let negative = false
  if (s.startsWith('(') && s.endsWith(')')) {
    negative = true
    s = s.slice(1, -1)
  }
  if (s.startsWith('-')) {
    negative = !negative
    s = s.slice(1)
  }
  if (!/^(\d+(\.\d{0,2})?|\.\d{1,2})$/.test(s)) return null
  const [whole, frac = ''] = s.split('.')
  const cents = Number(whole || '0') * 100 + Number((frac + '00').slice(0, 2))
  return negative ? -cents : cents
}

/** Cents as an editable dollar string: 123456 -> "1234.56" */
export function dollarsInput(cents: number): string {
  return (cents / 100).toFixed(2)
}

const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec']

export function monthName(m: number): string {
  return MONTHS[m - 1] ?? ''
}

/** "2026-11-12" -> "Nov 12" (adds the year when it differs from `currentYear`) */
export function shortDate(iso: string | null | undefined, currentYear?: number): string {
  if (!iso) return '—'
  const [y, m, d] = iso.split('-').map(Number)
  const base = `${MONTHS[m - 1]} ${d}`
  return currentYear !== undefined && y !== currentYear ? `${base}, ${y}` : base
}

/** "Nov 6 – 12" or "Dec 25 – Jan 1" */
export function dateRange(start: string, end: string): string {
  const [, sm] = start.split('-').map(Number)
  const [, em, ed] = end.split('-').map(Number)
  return sm === em ? `${shortDate(start)} – ${ed}` : `${shortDate(start)} – ${shortDate(end)}`
}

export function pct(value: number, digits = 0): string {
  return `${value.toFixed(digits)}%`
}

/** Twelfths as a readable split: 10 -> "10/12 this year, 2/12 next" */
export function splitText(twelfths: number | null): string {
  if (twelfths === null) return ''
  return `${twelfths}/12 this year, ${12 - twelfths}/12 next`
}

/** Default split for a payment arriving in `month`: months through December fund this year. */
export function defaultSplit(month: number): number {
  return 13 - Math.min(12, Math.max(1, month))
}

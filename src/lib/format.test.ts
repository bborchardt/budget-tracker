import { describe, expect, it } from 'vitest'
import { dateRange, defaultSplit, money, parseDollars, shortDate, splitText } from './format'

describe('money', () => {
  it('formats cents', () => {
    expect(money(123456)).toBe('$1,234.56')
    expect(money(-525)).toBe('−$5.25')
    expect(money(0)).toBe('$0.00')
    expect(money(1000, { signed: true })).toBe('+$10.00')
    expect(money(123456, { whole: true })).toBe('$1,235')
    expect(money(null)).toBe('—')
  })
})

describe('parseDollars', () => {
  it('parses what people type', () => {
    expect(parseDollars('1,234.56')).toBe(123456)
    expect(parseDollars('$12')).toBe(1200)
    expect(parseDollars('-5')).toBe(-500)
    expect(parseDollars('(5.00)')).toBe(-500)
    expect(parseDollars('.5')).toBe(50)
    expect(parseDollars('3.')).toBe(300)
    expect(parseDollars('  ')).toBeNull()
    expect(parseDollars('abc')).toBeNull()
    expect(parseDollars('1.234')).toBeNull()
  })
})

describe('dates', () => {
  it('formats short dates and ranges', () => {
    expect(shortDate('2026-11-12')).toBe('Nov 12')
    expect(shortDate('2025-12-30', 2026)).toBe('Dec 30, 2025')
    expect(dateRange('2026-11-06', '2026-11-12')).toBe('Nov 6 – 12')
    expect(dateRange('2026-01-30', '2026-02-05')).toBe('Jan 30 – Feb 5')
  })
})

describe('split', () => {
  it('defaults to the months from arrival through December', () => {
    expect(defaultSplit(3)).toBe(10)
    expect(defaultSplit(5)).toBe(8)
    expect(splitText(8)).toBe('8/12 this year, 4/12 next')
  })
})

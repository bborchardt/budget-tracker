<script lang="ts">
  import { api, errorText } from '../lib/api'
  import { app, changed } from '../lib/state.svelte'
  import { dollarsInput, money, parseDollars, shortDate } from '../lib/format'
  import { optionsFrom, sameTag, type TagOptions } from '../lib/tags'
  import type { Tag, Transaction, TransactionFilter, TxType } from '../lib/types'
  import Modal from '../components/Modal.svelte'
  import TagSelect from '../components/TagSelect.svelte'

  let rows = $state<Transaction[]>([])
  let total = $state(0)
  let options = $state<TagOptions>({ categories: [], income: [], savings: [] })
  let error = $state('')
  let busy = $state(false)

  let keyword = $state('')
  let categoryId = $state<number | null>(null)
  let kind = $state<TxType | ''>('')
  let autoOnly = $state(false)
  let pendingOnly = $state(false)
  let sort = $state<'date' | 'amount' | 'category' | 'type'>('date')
  let ascending = $state(false)
  let selected = $state<Set<number>>(new Set())

  // Filters requested by another view (e.g. clicking a category on the dashboard)
  $effect(() => {
    const f = app.transactionFilter
    if (f) {
      categoryId = f.category_id ?? null
      autoOnly = !!f.auto_only
      pendingOnly = !!f.pending_only
      app.transactionFilter = null
    }
  })

  $effect(() => {
    const filter: TransactionFilter = {
      year: app.year,
      keyword: keyword || undefined,
      category_id: categoryId ?? undefined,
      type: kind || undefined,
      auto_only: autoOnly,
      pending_only: pendingOnly,
      sort,
      ascending,
      limit: 2000,
    }
    void app.version
    load(filter)
  })

  async function load(filter: TransactionFilter) {
    try {
      const [page, overview, savings] = await Promise.all([
        api.listTransactions(filter),
        api.overview(filter.year!),
        api.listSavings(),
      ])
      rows = page.rows
      total = page.total
      options = optionsFrom(overview, savings.map((s) => s.account))
      selected = new Set([...selected].filter((id) => page.rows.some((r) => r.id === id)))
      error = ''
    } catch (e) {
      error = errorText(e)
    }
  }

  function tagOf(t: Transaction): Tag {
    return { type: t.type, category_id: t.category_id, income_source_id: t.income_source_id, savings_account_id: t.savings_account_id }
  }

  function setSort(s: typeof sort) {
    if (sort === s) ascending = !ascending
    else {
      sort = s
      ascending = s === 'category' || s === 'type'
    }
  }

  function toggle(id: number) {
    const next = new Set(selected)
    if (next.has(id)) next.delete(id)
    else next.add(id)
    selected = next
  }

  function toggleAll() {
    selected = selected.size === rows.length ? new Set() : new Set(rows.map((r) => r.id))
  }

  async function act(fn: () => Promise<unknown>) {
    busy = true
    try {
      await fn()
      error = ''
      changed()
    } catch (e) {
      error = errorText(e)
    } finally {
      busy = false
    }
  }

  // Re-tagging one row teaches the app for that merchant
  function retag(t: Transaction, tag: Tag | null) {
    if (!tag || sameTag(tag, tagOf(t))) return
    act(() => api.setTagBulk([t.id], tag, true))
  }

  let bulkTag = $state<Tag | null>(null)
  function applyBulk() {
    if (!bulkTag || selected.size === 0) return
    const tag = bulkTag
    act(async () => {
      await api.setTagBulk([...selected], tag, true)
      bulkTag = null
    })
  }

  function deleteSelected() {
    if (selected.size === 0) return
    if (!confirm(`Delete ${selected.size} transaction${selected.size === 1 ? '' : 's'}?`)) return
    act(async () => {
      await api.deleteTransactions([...selected])
      selected = new Set()
    })
  }

  // ---------- add / edit ----------

  type Edit = { id: number | null; date: string; description: string; amount: string; tag: Tag | null; notes: string; learn: boolean }
  let edit = $state<Edit | null>(null)
  let editError = $state('')

  async function newTransaction() {
    const today = await api.today()
    edit = { id: null, date: today, description: '', amount: '', tag: null, notes: '', learn: false }
    editError = ''
  }

  function editRow(t: Transaction) {
    edit = { id: t.id, date: t.date, description: t.display_name, amount: dollarsInput(t.amount_cents), tag: tagOf(t), notes: t.notes, learn: true }
    editError = ''
  }

  async function saveEdit(e: Edit) {
    const cents = parseDollars(e.amount)
    if (cents === null) return (editError = 'Enter an amount. Spending is negative, e.g. -42.10')
    if (!e.tag) return (editError = 'Choose a category, income source, or transfer')
    busy = true
    try {
      if (e.id === null) {
        await api.createTransaction({ date: e.date, description: e.description, amount_cents: cents, tag: e.tag, notes: e.notes })
      } else {
        await api.updateTransaction(e.id, { date: e.date, display_name: e.description, amount_cents: cents, tag: e.tag, notes: e.notes, learn: e.learn })
      }
      edit = null
      changed()
    } catch (err) {
      editError = errorText(err)
    } finally {
      busy = false
    }
  }

  function keydown(e: KeyboardEvent) {
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'n') {
      e.preventDefault()
      newTransaction()
    }
  }

  const sortMark = (s: typeof sort) => (sort === s ? (ascending ? ' ↑' : ' ↓') : '')
</script>

<svelte:window onkeydown={keydown} />

<div class="row" style="margin-bottom: 14px">
  <h1>Transactions</h1>
  <span class="muted">{total} in {app.year}</span>
  <span class="spacer"></span>
  <button class="primary" onclick={newTransaction}>New transaction <span class="faint">⌘N</span></button>
</div>

{#if error}<p class="notice error" style="margin-bottom: 12px">{error}</p>{/if}

<div class="section">
  <div class="section-head">
    <input type="search" placeholder="Search" bind:value={keyword} style="width: 200px" />
    <select bind:value={categoryId}>
      <option value={null}>All categories</option>
      {#each options.categories as c (c.id)}<option value={c.id}>{c.name}</option>{/each}
    </select>
    <select bind:value={kind}>
      <option value="">All types</option>
      <option value="expense">Spending</option>
      <option value="income">Income</option>
      <option value="transfer">Transfers</option>
    </select>
    <label><input type="checkbox" bind:checked={autoOnly} /> Auto-tagged</label>
    <label><input type="checkbox" bind:checked={pendingOnly} /> Pending</label>
    <span class="spacer"></span>
    {#if selected.size > 0}
      <span class="muted">{selected.size} selected</span>
      <TagSelect tag={bulkTag} {options} onchange={(t) => (bulkTag = t)} />
      <button disabled={!bulkTag || busy} onclick={applyBulk}>Tag</button>
      <button class="danger" disabled={busy} onclick={deleteSelected}>Delete</button>
    {/if}
  </div>
  {#if rows.length === 0}
    <p class="empty">No transactions match.</p>
  {:else}
    <table>
      <thead>
        <tr>
          <th style="width: 28px"><input type="checkbox" checked={selected.size === rows.length} onchange={toggleAll} aria-label="Select all" /></th>
          <th class="sortable" onclick={() => setSort('date')}>Date{sortMark('date')}</th>
          <th>Description</th>
          <th class="sortable" onclick={() => setSort('category')}>Tag{sortMark('category')}</th>
          <th class="num sortable" onclick={() => setSort('amount')}>Amount{sortMark('amount')}</th>
          <th>Source</th>
          <th></th>
        </tr>
      </thead>
      <tbody>
        {#each rows as t (t.id)}
          <tr class:selected={selected.has(t.id)}>
            <td><input type="checkbox" checked={selected.has(t.id)} onchange={() => toggle(t.id)} aria-label="Select" /></td>
            <td class="nowrap">{shortDate(t.date, app.year)}</td>
            <td>
              <span title={t.description}>{t.display_name}</span>
              {#if t.is_pending}<span class="badge pending">pending</span>{/if}
              {#if t.notes}<span class="faint small" title={t.notes}>✎</span>{/if}
            </td>
            <td>
              <TagSelect tag={tagOf(t)} {options} onchange={(tag) => retag(t, tag)} disabled={busy} />
              {#if t.is_auto}<span class="badge auto" title="Guessed. Re-tag to teach the app.">auto</span>{/if}
            </td>
            <td class="num" class:text-green={t.amount_cents > 0}>{money(t.amount_cents)}</td>
            <td class="muted small">{t.source_name ?? 'manual'}</td>
            <td><button class="link" onclick={() => editRow(t)}>Edit</button></td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</div>

{#if edit}
  {@const e = edit}
  <Modal title={e.id === null ? 'New transaction' : 'Edit transaction'} onclose={() => (edit = null)}>
    <form class="stack" onsubmit={(ev) => (ev.preventDefault(), saveEdit(e))}>
      <div class="row">
        <label class="field">Date <input type="date" bind:value={e.date} /></label>
        <label class="field">Amount <input class="money" bind:value={e.amount} placeholder="-42.10" /></label>
      </div>
      <label class="field">Description <input bind:value={e.description} /></label>
      <label class="field">Tag <TagSelect tag={e.tag} {options} onchange={(t) => (e.tag = t)} /></label>
      <p class="muted small">Negative = money out (spending, transfers to savings). Positive = refunds, income, money in.</p>
      <label class="field">Notes <input bind:value={e.notes} /></label>
      {#if e.id !== null}<label><input type="checkbox" bind:checked={e.learn} /> Use this tag for this merchant next time</label>{/if}
      {#if editError}<p class="notice error">{editError}</p>{/if}
      <div class="row"><span class="spacer"></span><button class="primary" disabled={busy}>Save</button></div>
    </form>
  </Modal>
{/if}

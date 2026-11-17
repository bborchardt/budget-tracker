<script lang="ts">
  // Savings accounts: balance from transfers, corrected by occasional balance checks.
  import { api, errorText } from '../lib/api'
  import { app, changed } from '../lib/state.svelte'
  import { money, parseDollars, shortDate } from '../lib/format'
  import type { AccountBalance, Snapshot } from '../lib/types'
  import Modal from '../components/Modal.svelte'

  let accounts = $state<AccountBalance[]>([])
  let history = $state<Record<number, Snapshot[]>>({})
  let expanded = $state<number | null>(null)
  let error = $state('')
  let busy = $state(false)
  let today = $state('')

  $effect(() => {
    void app.version
    load()
  })

  async function load() {
    try {
      ;[accounts, today] = await Promise.all([api.listSavings(), api.today()])
      if (expanded !== null) history[expanded] = await api.listSnapshots(expanded)
      error = ''
    } catch (e) {
      error = errorText(e)
    }
  }

  async function toggle(id: number) {
    expanded = expanded === id ? null : id
    if (expanded !== null) history[id] = await api.listSnapshots(id)
  }

  type Dialog =
    | { kind: 'account'; id: number | null; name: string; balance: string; date: string; archived: boolean }
    | { kind: 'snapshot'; account: AccountBalance; balance: string; date: string; note: string }
  let dialog = $state<Dialog | null>(null)
  let dialogError = $state('')

  async function submit(fn: () => Promise<unknown>) {
    busy = true
    try {
      await fn()
      dialog = null
      changed()
    } catch (e) {
      dialogError = errorText(e)
    } finally {
      busy = false
    }
  }

  function cents(text: string): number {
    const c = parseDollars(text)
    if (c === null) throw 'Enter a balance like 12,500.00'
    return c
  }

  const total = $derived(accounts.filter((a) => !a.account.archived).reduce((s, a) => s + a.balance_cents, 0))
</script>

<div class="row" style="margin-bottom: 14px">
  <h1>Savings</h1>
  <span class="muted">Total {money(total)}</span>
  <span class="spacer"></span>
  <button
    class="primary"
    onclick={() => ((dialog = { kind: 'account', id: null, name: '', balance: '', date: today, archived: false }), (dialogError = ''))}
  >Add account</button>
</div>

{#if error}<p class="notice error" style="margin-bottom: 12px">{error}</p>{/if}

<div class="section">
  {#if accounts.length === 0}
    <p class="empty">
      Add your savings accounts with their current balance. Transfers tagged to an account keep its balance current;
      checking the real balance now and then corrects for interest and anything missed.
    </p>
  {:else}
    <table>
      <thead>
        <tr><th>Account</th><th class="num">Balance</th><th class="num">From transfers</th><th class="num">Difference</th><th>Last checked</th><th></th></tr>
      </thead>
      <tbody>
        {#each accounts as a (a.account.id)}
          <tr class:faint={a.account.archived}>
            <td>
              <button class="link" onclick={() => toggle(a.account.id)}>{expanded === a.account.id ? '▾' : '▸'} {a.account.name}</button>
              {#if a.account.archived}<span class="badge">archived</span>{/if}
            </td>
            <td class="num">{money(a.balance_cents)}</td>
            <td class="num muted">{money(a.calculated_cents)}</td>
            <td class="num muted" title="Latest balance check minus what transfers predicted: interest or missed transfers">
              {a.drift_cents === null ? '—' : money(a.drift_cents, { signed: true })}
            </td>
            <td class="muted">{a.latest_snapshot ? shortDate(a.latest_snapshot.date, app.year) : 'never'}</td>
            <td class="num">
              <button
                onclick={() => ((dialog = { kind: 'snapshot', account: a, balance: '', date: today, note: '' }), (dialogError = ''))}
              >Check balance</button>
              <button
                class="link"
                onclick={() => (
                  (dialog = {
                    kind: 'account',
                    id: a.account.id,
                    name: a.account.name,
                    balance: (a.account.starting_balance_cents / 100).toFixed(2),
                    date: a.account.starting_date,
                    archived: a.account.archived,
                  }),
                  (dialogError = '')
                )}
              >Edit</button>
            </td>
          </tr>
          {#if expanded === a.account.id}
            <tr>
              <td colspan="6" class="history">
                <table>
                  <tbody>
                    <tr><td class="muted">{shortDate(a.account.starting_date)}</td><td>Starting balance</td><td class="num">{money(a.account.starting_balance_cents)}</td><td></td></tr>
                    {#each history[a.account.id] ?? [] as s (s.id)}
                      <tr>
                        <td class="muted">{shortDate(s.date, app.year)}</td>
                        <td>Balance check {s.note ? `· ${s.note}` : ''}</td>
                        <td class="num">{money(s.balance_cents)}</td>
                        <td class="num"><button class="link danger" onclick={() => api.deleteSnapshot(s.id).then(changed)}>Delete</button></td>
                      </tr>
                    {/each}
                  </tbody>
                </table>
              </td>
            </tr>
          {/if}
        {/each}
      </tbody>
    </table>
  {/if}
</div>

{#if dialog?.kind === 'account'}
  {@const d = dialog}
  <Modal title={d.id === null ? 'Add savings account' : 'Edit savings account'} onclose={() => (dialog = null)}>
    <form
      class="stack"
      onsubmit={(e) => (
        e.preventDefault(),
        submit(() =>
          d.id === null
            ? api.addSavingsAccount(d.name, cents(d.balance), d.date)
            : api.updateSavingsAccount(d.id, d.name, cents(d.balance), d.date, d.archived),
        )
      )}
    >
      <label class="field">Name <input bind:value={d.name} placeholder="e.g. Savings 1" /></label>
      <div class="row">
        <label class="field">Balance <input class="money" bind:value={d.balance} /></label>
        <label class="field">As of <input type="date" bind:value={d.date} /></label>
      </div>
      {#if d.id !== null}<label><input type="checkbox" bind:checked={d.archived} /> Archived (closed account)</label>{/if}
      {#if dialogError}<p class="notice error">{dialogError}</p>{/if}
      <div class="row"><span class="spacer"></span><button class="primary" disabled={busy}>Save</button></div>
    </form>
  </Modal>
{:else if dialog?.kind === 'snapshot'}
  {@const d = dialog}
  <Modal title={`Check ${d.account.account.name} balance`} onclose={() => (dialog = null)}>
    <form class="stack" onsubmit={(e) => (e.preventDefault(), submit(() => api.addSnapshot(d.account.account.id, d.date, cents(d.balance), d.note)))}>
      <p class="muted">Transfers say {money(d.account.balance_cents)}. Enter the balance the bank shows.</p>
      <div class="row">
        <label class="field">Balance <input class="money" bind:value={d.balance} /></label>
        <label class="field">As of <input type="date" bind:value={d.date} /></label>
      </div>
      <label class="field">Note (optional) <input bind:value={d.note} /></label>
      {#if dialogError}<p class="notice error">{dialogError}</p>{/if}
      <div class="row"><span class="spacer"></span><button class="primary" disabled={busy}>Save</button></div>
    </form>
  </Modal>
{/if}

<style>
  .history {
    background: var(--surface-2);
    padding: 0 0 0 28px;
  }
  .history table td {
    border-bottom: none;
    padding: 4px 10px;
  }
</style>

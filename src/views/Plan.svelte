<script lang="ts">
  // The annual plan: income − hold-back to savings + draws = money to budget, all assigned.
  import { api, errorText } from '../lib/api'
  import { app, changed } from '../lib/state.svelte'
  import { defaultSplit, dollarsInput, money, monthName, parseDollars, shortDate, splitText } from '../lib/format'
  import type { Adjustment, CategoryRow, IncomeRow, IncomeSourceInput, Overview, Plan } from '../lib/types'
  import Modal from '../components/Modal.svelte'

  let overview = $state<Overview | null>(null)
  let plans = $state<Plan[]>([])
  let log = $state<Adjustment[]>([])
  let error = $state('')
  let busy = $state(false)

  $effect(() => {
    const year = app.year
    void app.version
    load(year)
  })

  async function load(year: number) {
    try {
      const [o, p] = await Promise.all([api.overview(year), api.listPlans()])
      overview = o
      plans = p
      log = o.plan ? await api.adjustments(o.plan.id) : []
      error = ''
    } catch (e) {
      error = errorText(e)
    }
  }

  async function run(fn: () => Promise<unknown>): Promise<boolean> {
    busy = true
    try {
      await fn()
      error = ''
      changed()
      return true
    } catch (e) {
      error = errorText(e)
      return false
    } finally {
      busy = false
    }
  }

  const tracking = $derived(overview?.tracking ?? false)
  const previousPlan = $derived(plans.find((p) => p.year === app.year - 1) ?? null)

  // ---------- dialogs ----------

  type Dialog =
    | { kind: 'source'; source: IncomeRow | null; input: IncomeSourceInput; amount: string; split: boolean }
    | { kind: 'projection'; source: IncomeRow; amount: string; confirm: boolean; reason: string }
    | { kind: 'removeSource'; source: IncomeRow; moveTo: number | null; reason: string }
    | { kind: 'category'; category: CategoryRow | null; name: string; amount: string; lumpy: boolean }
    | { kind: 'limit'; category: CategoryRow; amount: string; reason: string }
    | { kind: 'move'; from: CategoryRow; toId: number | null; amount: string; reason: string }
    | { kind: 'removeCategory'; category: CategoryRow; mergeInto: number | null; moveLimit: boolean; reason: string }

  let dialog = $state<Dialog | null>(null)
  let dialogError = $state('')

  function open(d: Dialog) {
    dialog = d
    dialogError = ''
  }

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

  function amountOrError(text: string): number {
    const cents = parseDollars(text)
    if (cents === null || cents < 0) throw 'Enter an amount like 1,250 or 1250.00'
    return cents
  }

  function newSource() {
    open({
      kind: 'source',
      source: null,
      input: { name: '', timing: 'steady', expected_months: [], is_estimate: false, split_current_twelfths: null },
      amount: '',
      split: false,
    })
  }

  function editSource(s: IncomeRow) {
    open({
      kind: 'source',
      source: s,
      input: {
        name: s.name,
        timing: s.timing,
        expected_months: [...s.expected_months],
        is_estimate: s.is_estimate,
        split_current_twelfths: s.split_current_twelfths,
      },
      amount: dollarsInput(s.projected_cents),
      split: s.split_current_twelfths !== null,
    })
  }

  function toggleMonth(d: Extract<Dialog, { kind: 'source' }>, m: number) {
    d.input.expected_months = d.input.expected_months.includes(m)
      ? d.input.expected_months.filter((x) => x !== m)
      : [...d.input.expected_months, m].sort((a, b) => a - b)
  }

  function saveSource(d: Extract<Dialog, { kind: 'source' }>) {
    submit(async () => {
      const input: IncomeSourceInput = {
        ...d.input,
        expected_months: d.input.timing === 'lumpy' ? d.input.expected_months : [],
        split_current_twelfths: d.split
          ? (d.input.split_current_twelfths ?? defaultSplit(d.input.expected_months[0] ?? 1))
          : null,
      }
      if (d.source) {
        await api.updateIncomeSource(d.source.id, input)
      } else {
        await api.addIncomeSource(overview!.plan!.id, input, amountOrError(d.amount))
      }
    })
  }

  async function createPlan(copyFrom: number | null) {
    await run(() => api.createPlan(app.year, copyFrom))
  }

  // ---------- savings plan ----------

  let holdback = $state('')
  let draw = $state('')
  let savingsReason = $state('')
  $effect(() => {
    if (overview?.plan) {
      holdback = dollarsInput(overview.plan.planned_holdback_cents)
      draw = dollarsInput(overview.plan.planned_draw_cents)
    }
  })
  const savingsDirty = $derived(
    !!overview?.plan &&
      (parseDollars(holdback) !== overview.plan.planned_holdback_cents ||
        parseDollars(draw) !== overview.plan.planned_draw_cents),
  )

  async function saveSavings() {
    const h = parseDollars(holdback)
    const d = parseDollars(draw)
    if (h === null || d === null || h < 0 || d < 0) {
      error = 'Enter savings amounts like 10,000 or 0'
      return
    }
    if (await run(() => api.setSavingsPlan(overview!.plan!.id, h, d, savingsReason))) savingsReason = ''
  }

  // ---------- adjustment log text ----------

  function describe(a: Adjustment): string {
    const change = (o: number | null, n: number | null) => `${money(o, { whole: true })} → ${money(n, { whole: true })}`
    switch (a.kind) {
      case 'category_limit':
        return `${a.category_name}: ${change(a.old_cents, a.new_cents)}`
      case 'category_added':
        return `Added ${a.category_name} (${money(a.new_cents, { whole: true })})`
      case 'category_removed':
        return `Removed ${a.category_name}${a.to_category_name ? ` → merged into ${a.to_category_name}` : ''}${
          a.amount_cents ? `, ${money(a.amount_cents, { whole: true })} budget moved` : ''
        } (${a.detail})`
      case 'reallocation':
        return `Moved ${money(a.amount_cents, { whole: true })} ${a.category_name} → ${a.to_category_name}`
      case 'income_projection':
        return `${a.income_source_name} projection: ${change(a.old_cents, a.new_cents)}`
      case 'income_added':
        return `Added income ${a.income_source_name} (${money(a.new_cents, { whole: true })})`
      case 'income_removed':
        return `Removed income ${a.income_source_name} (${a.detail})`
      case 'savings_holdback':
        return `Savings hold-back: ${change(a.old_cents, a.new_cents)}`
      case 'savings_draw':
        return `Draw from savings: ${change(a.old_cents, a.new_cents)}`
      default:
        return a.kind
    }
  }

  function timingText(s: IncomeRow): string {
    if (s.timing === 'steady') return 'Steady'
    return s.expected_months.length ? `Expected ${s.expected_months.map(monthName).join(', ')}` : 'Lumpy'
  }
</script>

{#if error}<p class="notice error" style="margin-bottom: 12px">{error}</p>{/if}

{#if !overview}
  <p class="muted">Loading…</p>
{:else if !overview.plan}
  <div class="section">
    <div class="empty stack">
      <h2>Plan for {app.year}</h2>
      <p>
        A plan sets the year's projected take-home income, what goes to or comes from savings, and how the rest is
        budgeted across categories. Misc and Other / Gifts are built in.
      </p>
      <div class="row" style="justify-content: center">
        {#if previousPlan}
          <button class="primary" disabled={busy} onclick={() => createPlan(previousPlan.year)}>
            Copy {previousPlan.year}'s plan
          </button>
        {/if}
        <button class={previousPlan ? '' : 'primary'} disabled={busy} onclick={() => createPlan(null)}>Start blank</button>
      </div>
    </div>
  </div>
{:else}
  {@const o = overview}
  {@const t = o.totals!}
  <div class="row" style="margin-bottom: 14px">
    <h1>{o.year} plan</h1>
    {#if tracking}
      <span class="muted small">Changes are now recorded in the change log.</span>
    {:else}
      <span class="muted small">Setting up: changes aren't logged until the first transaction.</span>
    {/if}
  </div>

  <div class="section">
    <div class="section-head"><h2>Balance</h2></div>
    <table class="balance">
      <tbody>
        <tr><td>Projected take-home income</td><td class="num">{money(t.projected_income_cents, { whole: true })}</td></tr>
        <tr><td>− Held back to savings</td><td class="num">{money(-t.planned_holdback_cents, { whole: true })}</td></tr>
        {#if t.auto_holdback_cents}
          <tr><td class="muted">− Next year's share of split income (to savings)</td><td class="num">{money(-t.auto_holdback_cents, { whole: true })}</td></tr>
        {/if}
        <tr><td>+ Drawn from savings</td><td class="num">{money(t.planned_draw_cents, { whole: true })}</td></tr>
        {#if t.carried_in_cents}
          <tr><td class="muted">+ Carried in from last year's split income</td><td class="num">{money(t.carried_in_cents, { whole: true })}</td></tr>
        {/if}
        <tr class="strong"><td>= Available to budget</td><td class="num">{money(t.available_cents, { whole: true })}</td></tr>
        <tr><td>Assigned to categories</td><td class="num">{money(t.assigned_cents, { whole: true })}</td></tr>
        <tr class="strong">
          <td>{t.unassigned_cents >= 0 ? 'Not yet assigned' : 'Over-assigned'}</td>
          <td class="num" class:text-green={t.unassigned_cents === 0} class:text-red={t.unassigned_cents !== 0}>
            {money(Math.abs(t.unassigned_cents), { whole: true })}{t.unassigned_cents === 0 ? ' ✓' : ''}
          </td>
        </tr>
      </tbody>
    </table>
  </div>

  <div class="section">
    <div class="section-head">
      <h2>Income</h2>
      <span class="muted small">Take-home pay, by source</span>
      <span class="spacer"></span>
      <button onclick={newSource}>Add source</button>
    </div>
    <table>
      <thead>
        <tr><th>Source</th><th>Timing</th><th class="num">Projected</th><th class="num">Received</th><th></th></tr>
      </thead>
      <tbody>
        {#each o.income as s (s.id)}
          <tr>
            <td>
              {s.name}
              {#if s.is_estimate}<span class="badge">estimate</span>{/if}
              {#if s.is_other}<span class="faint small">built in</span>{/if}
            </td>
            <td class="muted">
              {timingText(s)}
              {#if s.split_current_twelfths !== null}
                <div class="small">Split: {splitText(s.split_current_twelfths)} ({money(s.current_share_cents, { whole: true })} / {money(s.next_share_cents, { whole: true })})</div>
              {/if}
            </td>
            <td class="num">{money(s.projected_cents, { whole: true })}</td>
            <td class="num">{money(s.actual_cents, { whole: true })}</td>
            <td class="num">
              {#if s.needs_confirmation}
                <button
                  class="primary"
                  onclick={() => open({ kind: 'projection', source: s, amount: dollarsInput(s.actual_cents), confirm: true, reason: '' })}
                >Confirm actual</button>
              {/if}
              <button
                class="link"
                onclick={() => open({ kind: 'projection', source: s, amount: dollarsInput(s.projected_cents), confirm: false, reason: '' })}
              >Change amount</button>
              {#if !s.is_other}
                <button class="link" onclick={() => editSource(s)}>Edit</button>
                <button class="link danger" onclick={() => open({ kind: 'removeSource', source: s, moveTo: null, reason: '' })}>Remove</button>
              {/if}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>

  <div class="section">
    <div class="section-head"><h2>Savings plan</h2></div>
    <div class="section-body row" style="align-items: flex-end">
      <label class="field">Hold back to savings <input class="money" bind:value={holdback} /></label>
      <label class="field">Draw from savings <input class="money" bind:value={draw} /></label>
      {#if tracking && savingsDirty}
        <label class="field" style="flex: 1">Reason (optional) <input bind:value={savingsReason} /></label>
      {/if}
      <button class="primary" disabled={!savingsDirty || busy} onclick={saveSavings}>Save</button>
    </div>
  </div>

  <div class="section">
    <div class="section-head">
      <h2>Spending categories</h2>
      <span class="spacer"></span>
      <button onclick={() => open({ kind: 'category', category: null, name: '', amount: '', lumpy: false })}>Add category</button>
    </div>
    <table>
      <thead>
        <tr><th>Category</th><th class="num">Annual budget</th><th class="num">Spent</th><th></th></tr>
      </thead>
      <tbody>
        {#each o.categories as c (c.id)}
          <tr>
            <td>
              {c.name}
              {#if c.is_lumpy}<span class="badge">lumpy</span>{/if}
              {#if c.is_misc}<span class="faint small">built in · catches anything unrecognized</span>{/if}
            </td>
            <td class="num">{money(c.annual_limit_cents, { whole: true })}</td>
            <td class="num muted">{money(c.progress.spent_cents, { whole: true })}</td>
            <td class="num">
              <button class="link" onclick={() => open({ kind: 'limit', category: c, amount: dollarsInput(c.annual_limit_cents), reason: '' })}>Change budget</button>
              <button class="link" onclick={() => open({ kind: 'move', from: c, toId: null, amount: '', reason: '' })}>Move budget</button>
              <button
                class="link"
                onclick={() => open({ kind: 'category', category: c, name: c.name, amount: '', lumpy: c.is_lumpy })}
              >Edit</button>
              {#if !c.is_misc}
                <button
                  class="link danger"
                  onclick={() => open({ kind: 'removeCategory', category: c, mergeInto: o.categories.find((x) => x.is_misc)?.id ?? null, moveLimit: true, reason: '' })}
                >Remove</button>
              {/if}
            </td>
          </tr>
        {/each}
      </tbody>
      <tfoot>
        <tr><td>Total</td><td class="num">{money(t.assigned_cents, { whole: true })}</td><td class="num">{money(o.spending.spent_cents, { whole: true })}</td><td></td></tr>
      </tfoot>
    </table>
  </div>

  <div class="section">
    <div class="section-head"><h2>Change log</h2></div>
    {#if log.length === 0}
      <p class="empty">No changes since tracking started.</p>
    {:else}
      <table>
        <tbody>
          {#each log as a (a.id)}
            <tr>
              <td class="nowrap muted" style="width: 90px">{shortDate(a.date, o.year)}</td>
              <td>{describe(a)}</td>
              <td class="muted">{a.reason}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}
  </div>
{/if}

{#if dialog && overview?.plan}
  {@const o = overview}
  {#if dialog.kind === 'source'}
    {@const d = dialog}
    <Modal title={d.source ? `Edit ${d.source.name}` : 'Add income source'} onclose={() => (dialog = null)}>
      <form class="stack" onsubmit={(e) => (e.preventDefault(), saveSource(d))}>
        <label class="field">Name <input bind:value={d.input.name} placeholder="e.g. Salary" /></label>
        {#if !d.source}
          <label class="field">Projected take-home for the year <input class="money" bind:value={d.amount} /></label>
        {/if}
        <div class="row">
          <label><input type="radio" bind:group={d.input.timing} value="steady" /> Steady (paychecks)</label>
          <label><input type="radio" bind:group={d.input.timing} value="lumpy" /> Arrives in specific months</label>
        </div>
        {#if d.input.timing === 'lumpy'}
          <div class="row months">
            {#each Array.from({ length: 12 }, (_, i) => i + 1) as m (m)}
              <label><input type="checkbox" checked={d.input.expected_months.includes(m)} onchange={() => toggleMonth(d, m)} />{monthName(m)}</label>
            {/each}
          </div>
        {/if}
        <label><input type="checkbox" bind:checked={d.input.is_estimate} /> This amount is an estimate</label>
        <label><input type="checkbox" bind:checked={d.split} /> Split across this year and next (e.g. an annual bonus)</label>
        {#if d.split}
          <label class="field">
            Twelfths that fund this year
            <select
              value={d.input.split_current_twelfths ?? defaultSplit(d.input.expected_months[0] ?? 1)}
              onchange={(e) => (d.input.split_current_twelfths = Number((e.currentTarget as HTMLSelectElement).value))}
            >
              {#each Array.from({ length: 13 }, (_, i) => 12 - i) as tw (tw)}
                <option value={tw}>{splitText(tw)}</option>
              {/each}
            </select>
          </label>
          <p class="muted small">Next year's share is held back in savings automatically and carried into next year's plan.</p>
        {/if}
        {#if dialogError}<p class="notice error">{dialogError}</p>{/if}
        <div class="row"><span class="spacer"></span><button class="primary" disabled={busy}>Save</button></div>
      </form>
    </Modal>
  {:else if dialog.kind === 'projection'}
    {@const d = dialog}
    <Modal title={d.confirm ? `Confirm ${d.source.name}` : `Change ${d.source.name}`} onclose={() => (dialog = null)}>
      <form class="stack" onsubmit={(e) => (e.preventDefault(), submit(() => api.setIncomeProjection(d.source.id, amountOrError(d.amount), d.confirm, d.reason)))}>
        {#if d.confirm}
          <p>
            Estimated {money(d.source.projected_cents)}; received {money(d.source.actual_cents)}. Confirming updates the plan to the
            actual amount{#if d.source.split_current_twelfths !== null} and next year's carry-forward{/if}.
          </p>
        {/if}
        <label class="field">{d.confirm ? 'Actual amount' : 'Projected for the year'} <input class="money" bind:value={d.amount} /></label>
        {#if !d.confirm}<label><input type="checkbox" bind:checked={d.confirm} /> This is the actual amount (no longer an estimate)</label>{/if}
        {#if tracking}<label class="field">Reason (optional) <input bind:value={d.reason} /></label>{/if}
        {#if dialogError}<p class="notice error">{dialogError}</p>{/if}
        <div class="row"><span class="spacer"></span><button class="primary" disabled={busy}>Save</button></div>
      </form>
    </Modal>
  {:else if dialog.kind === 'removeSource'}
    {@const d = dialog}
    <Modal title={`Remove ${d.source.name}`} onclose={() => (dialog = null)}>
      <form class="stack" onsubmit={(e) => (e.preventDefault(), submit(() => api.removeIncomeSource(d.source.id, d.moveTo, d.reason)))}>
        <label class="field">
          Move its income to
          <select bind:value={d.moveTo}>
            <option value={null}>(nothing to move)</option>
            {#each o.income.filter((s) => s.id !== d.source.id) as s (s.id)}<option value={s.id}>{s.name}</option>{/each}
          </select>
        </label>
        {#if tracking}<label class="field">Reason (optional) <input bind:value={d.reason} /></label>{/if}
        {#if dialogError}<p class="notice error">{dialogError}</p>{/if}
        <div class="row"><span class="spacer"></span><button class="primary" disabled={busy}>Remove</button></div>
      </form>
    </Modal>
  {:else if dialog.kind === 'category'}
    {@const d = dialog}
    <Modal title={d.category ? `Edit ${d.category.name}` : 'Add category'} onclose={() => (dialog = null)}>
      <form
        class="stack"
        onsubmit={(e) => (
          e.preventDefault(),
          submit(() =>
            d.category
              ? api.updateCategory(d.category.id, d.name, d.lumpy)
              : api.addCategory(o.plan!.id, d.name, amountOrError(d.amount || '0'), d.lumpy),
          )
        )}
      >
        <label class="field">Name <input bind:value={d.name} disabled={d.category?.is_misc} placeholder="e.g. Groceries" /></label>
        {#if !d.category}<label class="field">Annual budget <input class="money" bind:value={d.amount} /></label>{/if}
        <label>
          <input type="checkbox" bind:checked={d.lumpy} /> Lumpy: spent in bursts (e.g. vacation, gifts)
        </label>
        <p class="muted small">Lumpy categories aren't paced through the year; they only warn at 90% and when over.</p>
        {#if dialogError}<p class="notice error">{dialogError}</p>{/if}
        <div class="row"><span class="spacer"></span><button class="primary" disabled={busy}>Save</button></div>
      </form>
    </Modal>
  {:else if dialog.kind === 'limit'}
    {@const d = dialog}
    <Modal title={`Change ${d.category.name} budget`} onclose={() => (dialog = null)}>
      <form class="stack" onsubmit={(e) => (e.preventDefault(), submit(() => api.setCategoryLimit(d.category.id, amountOrError(d.amount), d.reason)))}>
        <label class="field">Annual budget (currently {money(d.category.annual_limit_cents, { whole: true })}) <input class="money" bind:value={d.amount} /></label>
        {#if tracking}<label class="field">Reason (optional) <input bind:value={d.reason} /></label>{/if}
        <p class="muted small">Pace is recalculated against the new budget for the whole year.</p>
        {#if dialogError}<p class="notice error">{dialogError}</p>{/if}
        <div class="row"><span class="spacer"></span><button class="primary" disabled={busy}>Save</button></div>
      </form>
    </Modal>
  {:else if dialog.kind === 'move'}
    {@const d = dialog}
    <Modal title={`Move budget from ${d.from.name}`} onclose={() => (dialog = null)}>
      <form
        class="stack"
        onsubmit={(e) => (
          e.preventDefault(),
          submit(async () => {
            if (d.toId === null) throw 'Choose a category to move budget to'
            await api.reallocate(d.from.id, d.toId, amountOrError(d.amount), d.reason)
          })
        )}
      >
        <div class="row">
          <label class="field">Amount <input class="money" bind:value={d.amount} /></label>
          <label class="field" style="flex: 1">
            To
            <select bind:value={d.toId}>
              <option value={null}>Choose…</option>
              {#each o.categories.filter((c) => c.id !== d.from.id) as c (c.id)}<option value={c.id}>{c.name}</option>{/each}
            </select>
          </label>
        </div>
        <p class="muted small">{d.from.name} has {money(d.from.annual_limit_cents, { whole: true })}. The plan total doesn't change.</p>
        {#if tracking}<label class="field">Reason (optional) <input bind:value={d.reason} /></label>{/if}
        {#if dialogError}<p class="notice error">{dialogError}</p>{/if}
        <div class="row"><span class="spacer"></span><button class="primary" disabled={busy}>Move</button></div>
      </form>
    </Modal>
  {:else if dialog.kind === 'removeCategory'}
    {@const d = dialog}
    <Modal title={`Remove ${d.category.name}`} onclose={() => (dialog = null)}>
      <form class="stack" onsubmit={(e) => (e.preventDefault(), submit(() => api.removeCategory(d.category.id, d.mergeInto, d.moveLimit, d.reason)))}>
        <label class="field">
          Move its transactions into
          <select bind:value={d.mergeInto}>
            {#each o.categories.filter((c) => c.id !== d.category.id) as c (c.id)}<option value={c.id}>{c.name}</option>{/each}
          </select>
        </label>
        <label><input type="checkbox" bind:checked={d.moveLimit} /> Also move its {money(d.category.annual_limit_cents, { whole: true })} budget</label>
        {#if tracking}<label class="field">Reason (optional) <input bind:value={d.reason} /></label>{/if}
        <p class="muted small">Past years aren't affected. Future transactions from the same merchants go to the new category.</p>
        {#if dialogError}<p class="notice error">{dialogError}</p>{/if}
        <div class="row"><span class="spacer"></span><button class="primary" disabled={busy}>Remove</button></div>
      </form>
    </Modal>
  {/if}
{/if}

<style>
  .balance td:first-child {
    width: 70%;
  }
  .balance tr.strong td {
    font-weight: 600;
  }
  .months label {
    min-width: 58px;
  }
</style>

<script lang="ts">
  // Where things stand: spending vs. pace, income vs. projection, savings vs. plan, last week.
  import { api, errorText } from '../lib/api'
  import { app, go } from '../lib/state.svelte'
  import { dateRange, money, pct, shortDate } from '../lib/format'
  import type { Overview } from '../lib/types'
  import StatusDot from '../components/StatusDot.svelte'

  let overview = $state<Overview | null>(null)
  let error = $state('')

  $effect(() => {
    const year = app.year
    void app.version
    api.overview(year).then(
      (o) => ((overview = o), (error = '')),
      (e) => (error = errorText(e)),
    )
  })

  const recentWeeks = $derived(overview ? overview.weeks.slice(-12).reverse() : [])
  const maxWeek = $derived(Math.max(1, ...recentWeeks.map((w) => w.spent_cents)))

  function showCategory(id: number) {
    app.transactionFilter = { category_id: id }
    go('transactions')
  }
  function showAuto() {
    app.transactionFilter = { auto_only: true }
    go('transactions')
  }
  function showPending() {
    app.transactionFilter = { pending_only: true }
    go('transactions')
  }
</script>

{#if error}
  <p class="notice error">{error}</p>
{:else if !overview}
  <p class="muted">Loading…</p>
{:else if !overview.plan}
  <div class="section">
    <div class="empty stack">
      <h2>No plan for {overview.year} yet</h2>
      <p>Start by setting up the year's income, savings, and spending categories.</p>
      <div><button class="primary" onclick={() => go('plan')}>Set up {overview.year}</button></div>
    </div>
  </div>
{:else}
  {@const o = overview}
  {@const t = o.totals!}
  <div class="row" style="margin-bottom: 14px">
    <h1>{o.year}</h1>
    {#if o.current_week}
      <span class="muted">
        Week {o.current_week.index} · {dateRange(o.current_week.start, o.current_week.end)} ·
        {pct(o.fraction_elapsed * 100)} of the year
      </span>
    {:else}
      <span class="muted">as of {shortDate(o.as_of, o.year)}</span>
    {/if}
    <span class="spacer"></span>
    {#if o.pending_count > 0}
      <button class="badge pending" onclick={showPending}>{o.pending_count} pending</button>
    {/if}
    {#if o.auto_count > 0}
      <button class="badge auto" onclick={showAuto} title="Tags the app guessed. Re-tag any you like; it learns.">
        {o.auto_count} auto-tagged
      </button>
    {/if}
    <button class="primary" onclick={() => go('import')}>Import</button>
  </div>

  {#if t.unassigned_cents !== 0}
    <p class="notice" style="margin-bottom: 14px">
      {#if t.unassigned_cents > 0}
        {money(t.unassigned_cents)} of the plan isn't assigned to a category yet.
      {:else}
        Categories are over-assigned by {money(-t.unassigned_cents)}.
      {/if}
      <button class="link" onclick={() => go('plan')}>Balance the plan</button>
    </p>
  {/if}

  <div class="stats">
    <div class="stat">
      <div class="label">Last week</div>
      <div class="value">{money(o.last_week?.spent_cents ?? 0, { whole: true })}</div>
      <div class="sub">
        {#if o.last_week}{dateRange(o.last_week.week.start, o.last_week.week.end)}{#if o.last_week.week.partial} (partial){/if}{:else}—{/if}
      </div>
    </div>
    <div class="stat">
      <div class="label">Spent this year</div>
      <div class="value">{money(o.spending.spent_cents, { whole: true })}</div>
      <div class="sub">of {money(o.spending.limit_cents, { whole: true })} · {money(o.spending.remaining_cents, { whole: true })} left</div>
    </div>
    <div class="stat">
      <div class="label">Income received</div>
      <div class="value">{money(o.income_totals.actual_cents, { whole: true })}</div>
      <div class="sub">
        expected {money(o.income_totals.expected_cents, { whole: true })} by now ·
        <span class:text-red={o.income_totals.actual_cents < o.income_totals.expected_cents}>
          {money(o.income_totals.actual_cents - o.income_totals.expected_cents, { whole: true, signed: true })}
        </span>
      </div>
    </div>
    <div class="stat">
      <div class="label">Saved this year</div>
      <div class="value">{money(o.savings.saved_cents - o.savings.drawn_cents, { whole: true })}</div>
      <div class="sub">
        net of {money(o.savings.planned_holdback_cents - o.savings.planned_draw_cents, { whole: true })} planned ·
        expected {money(o.savings.expected_saved_cents - o.savings.expected_drawn_cents, { whole: true })} by now
      </div>
    </div>
  </div>

  <div class="section">
    <div class="section-head"><h2>Spending by category</h2></div>
    <table>
      <thead>
        <tr>
          <th>Category</th>
          <th class="num">Spent</th>
          <th class="num">Budget</th>
          <th class="num">Remaining</th>
          <th style="width: 18%">Used</th>
          <th class="num" title="Average weekly spending that would land exactly on budget">Per week left</th>
        </tr>
      </thead>
      <tbody>
        {#each o.categories as c (c.id)}
          {@const p = c.progress}
          <tr>
            <td>
              <StatusDot status={p.status} />
              <button class="link" onclick={() => showCategory(c.id)}>{c.name}</button>
              {#if c.is_lumpy}<span class="badge" title="No pacing: only warns near or over the limit">lumpy</span>{/if}
              {#if c.limit_changed_on}<span class="faint small" title="Budget changed">↕ {shortDate(c.limit_changed_on)}</span>{/if}
            </td>
            <td class="num">{money(p.spent_cents)}</td>
            <td class="num">{money(p.limit_cents, { whole: true })}</td>
            <td class="num">
              {#if p.over_cents > 0}<span class="text-red">{money(p.over_cents)} over</span>{:else}{money(p.remaining_cents)}{/if}
            </td>
            <td>
              <div class="row" style="flex-wrap: nowrap">
                <div class="bar" style="flex: 1"><span class={p.status} style="width: {Math.min(100, Math.max(0, p.pct_used))}%"></span></div>
                <span class="small muted num" style="width: 38px">{pct(p.pct_used)}</span>
              </div>
            </td>
            <td class="num muted">{p.weekly_target_cents === null ? '—' : money(p.weekly_target_cents, { whole: true })}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>

  <div class="grid2">
    <div class="section">
      <div class="section-head"><h2>Income</h2></div>
      <table>
        <thead>
          <tr><th>Source</th><th class="num">Received</th><th class="num">Expected by now</th><th class="num">Projected</th></tr>
        </thead>
        <tbody>
          {#each o.income as s (s.id)}
            <tr>
              <td>
                {s.name}
                {#if s.is_estimate}<span class="badge">estimate</span>{/if}
                {#if s.needs_confirmation}<button class="badge warn" onclick={() => go('plan')}>confirm actual</button>{/if}
              </td>
              <td class="num">{money(s.actual_cents, { whole: true })}</td>
              <td class="num muted">{money(s.expected_cents, { whole: true })}</td>
              <td class="num">{money(s.projected_cents, { whole: true })}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>

    <div class="section">
      <div class="section-head">
        <h2>Spending by week</h2>
        <span class="spacer"></span>
        <span class="faint small">Fri – Thu</span>
      </div>
      <table>
        <tbody>
          {#each recentWeeks as w (w.week.index)}
            <tr>
              <td class="nowrap">
                {dateRange(w.week.start, w.week.end)}
                {#if o.current_week && w.week.index === o.current_week.index}<span class="badge">this week</span>{/if}
                {#if w.week.partial}<span class="faint small">partial</span>{/if}
              </td>
              <td style="width: 45%">
                <div class="bar"><span class="green" style="width: {Math.max(0, (w.spent_cents / maxWeek) * 100)}%"></span></div>
              </td>
              <td class="num">{money(w.spent_cents, { whole: true })}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  </div>

  {#if o.savings.accounts.length > 0}
    <div class="section">
      <div class="section-head">
        <h2>Savings</h2>
        <span class="spacer"></span>
        <span class="muted">Total {money(o.savings.total_balance_cents, { whole: true })}</span>
      </div>
      <table>
        <tbody>
          {#each o.savings.accounts as a (a.account.id)}
            <tr>
              <td>{a.account.name}</td>
              <td class="num">{money(a.balance_cents)}</td>
              <td class="muted small">
                {#if a.latest_snapshot}balance checked {shortDate(a.latest_snapshot.date, o.year)}{:else}no balance check yet{/if}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
{/if}

<style>
  .grid2 {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(380px, 1fr));
    gap: 0 16px;
  }
  button.badge {
    background: none;
    cursor: pointer;
  }
</style>

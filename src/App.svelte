<script lang="ts">
  import { app, go, type View } from './lib/state.svelte'
  import Dashboard from './views/Dashboard.svelte'
  import PlanView from './views/Plan.svelte'
  import Transactions from './views/Transactions.svelte'
  import Import from './views/Import.svelte'
  import Savings from './views/Savings.svelte'
  import SettingsView from './views/Settings.svelte'

  const nav: { view: View; label: string; key: string }[] = [
    { view: 'dashboard', label: 'Dashboard', key: '1' },
    { view: 'plan', label: 'Plan', key: '2' },
    { view: 'transactions', label: 'Transactions', key: '3' },
    { view: 'import', label: 'Import', key: '4' },
    { view: 'savings', label: 'Savings', key: '5' },
    { view: 'settings', label: 'Settings', key: '6' },
  ]

  const thisYear = new Date().getFullYear()
  const years = [thisYear - 2, thisYear - 1, thisYear, thisYear + 1]

  // ⌘1–6 switch views, ⌘I opens Import
  function keydown(e: KeyboardEvent) {
    if (!(e.metaKey || e.ctrlKey) || e.altKey) return
    const item = nav.find((n) => n.key === e.key)
    if (item) {
      e.preventDefault()
      go(item.view)
    } else if (e.key.toLowerCase() === 'i') {
      e.preventDefault()
      go('import')
    }
  }
</script>

<svelte:window onkeydown={keydown} />

<div class="shell">
  <nav>
    <div class="brand">Budget Tracker</div>
    {#each nav as item (item.view)}
      <button class="nav-item" class:active={app.view === item.view} onclick={() => go(item.view)}>
        <span>{item.label}</span>
        <span class="key">⌘{item.key}</span>
      </button>
    {/each}
    <div class="spacer"></div>
    <label class="field year">
      Year
      <select bind:value={app.year}>
        {#each years as y (y)}
          <option value={y}>{y}</option>
        {/each}
      </select>
    </label>
  </nav>
  <main>
    {#if app.view === 'dashboard'}
      <Dashboard />
    {:else if app.view === 'plan'}
      <PlanView />
    {:else if app.view === 'transactions'}
      <Transactions />
    {:else if app.view === 'import'}
      <Import />
    {:else if app.view === 'savings'}
      <Savings />
    {:else}
      <SettingsView />
    {/if}
  </main>
</div>

<style>
  .shell {
    display: flex;
    height: 100vh;
  }
  nav {
    width: 190px;
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 16px 10px;
    border-right: 1px solid var(--border);
    background: var(--surface-2);
  }
  .brand {
    font-weight: 600;
    padding: 0 8px 14px;
  }
  .nav-item {
    display: flex;
    justify-content: space-between;
    border: none;
    background: none;
    text-align: left;
    padding: 6px 8px;
  }
  .nav-item.active {
    background: var(--surface);
    box-shadow: var(--shadow);
    font-weight: 500;
  }
  .key {
    color: var(--text-3);
    font-size: 11px;
  }
  .year {
    padding: 0 8px;
  }
  main {
    flex: 1;
    overflow: auto;
    padding: 20px 24px 40px;
  }
</style>

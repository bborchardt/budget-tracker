<script lang="ts">
  import { api, errorText } from '../lib/api'
  import { changed } from '../lib/state.svelte'
  import type { Settings } from '../lib/types'

  let settings = $state<Settings | null>(null)
  let message = $state('')
  let error = $state('')

  $effect(() => {
    api.getSettings().then((s) => (settings = s), (e) => (error = errorText(e)))
  })

  async function save() {
    if (!settings) return
    try {
      settings = await api.saveSettings($state.snapshot(settings))
      message = 'Saved'
      error = ''
      changed()
    } catch (e) {
      error = errorText(e)
      message = ''
    }
  }
</script>

<div class="row" style="margin-bottom: 14px"><h1>Settings</h1></div>

{#if settings}
  <form class="stack" onsubmit={(e) => (e.preventDefault(), save())}>
    <div class="section">
      <div class="section-head"><h2>Pace</h2></div>
      <div class="section-body stack">
        <label class="field" style="max-width: 360px">
          Red when ahead of pace by (percentage points of the annual budget)
          <input type="number" min="0" max="100" step="1" bind:value={settings.pace_warning_threshold_pct} />
        </label>
        <label class="field" style="max-width: 360px">
          Lumpy categories turn yellow at (% of budget used)
          <input type="number" min="0" max="100" step="1" bind:value={settings.lumpy_warning_pct} />
        </label>
      </div>
    </div>

    <div class="section">
      <div class="section-head"><h2>Import</h2></div>
      <div class="section-body stack">
        <label class="field" style="max-width: 360px">
          Remove pending items that haven't posted after (days)
          <input type="number" min="1" step="1" bind:value={settings.pending_stale_days} />
        </label>
      </div>
    </div>

    <div class="section">
      <div class="section-head"><h2>AI assist (optional)</h2></div>
      <div class="section-body stack">
        <p class="muted">
          Uses a model server running on this computer to help with unfamiliar layouts and new merchants. Only local
          endpoints (localhost) are allowed; transaction data never leaves this machine. Not wired into import yet.
        </p>
        <label class="field" style="max-width: 420px">
          Local endpoint
          <input bind:value={settings.ai_endpoint} placeholder="http://localhost:8080/v1" />
        </label>
        <label><input type="checkbox" bind:checked={settings.ai_enabled} /> Enable AI assist</label>
      </div>
    </div>

    {#if error}<p class="notice error">{error}</p>{/if}
    {#if message}<p class="notice ok">{message}</p>{/if}
    <div><button class="primary">Save settings</button></div>
  </form>
{/if}

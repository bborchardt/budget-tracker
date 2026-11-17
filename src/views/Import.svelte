<script lang="ts">
  // Weekly import: paste (or open a CSV) per source, review one table, commit, undo if needed.
  import { api, errorText } from '../lib/api'
  import { app, changed, go } from '../lib/state.svelte'
  import { money, shortDate } from '../lib/format'
  import { optionsFrom, sameTag, type TagOptions } from '../lib/tags'
  import type { Batch, CommitRow, ImportSource, ImportSummary, Preview, PreviewRow, SavingsAccount, Tag } from '../lib/types'
  import TagSelect from '../components/TagSelect.svelte'

  interface Row {
    key: string
    blockId: number
    sourceId: number
    row: PreviewRow
    action: 'import' | 'skip' | 'replace_pending'
    tag: Tag | null
  }
  interface Block {
    id: number
    sourceId: number | null
    text: string
    preview: Preview | null
    /** Sign override chosen in review (null = remembered or detected) */
    flip: boolean | null
    error: string
  }

  let sources = $state<ImportSource[]>([])
  let savings = $state<SavingsAccount[]>([])
  let optionsByYear = $state<Record<number, TagOptions>>({})
  let blocks = $state<Block[]>([])
  let rows = $state<Row[]>([])
  let batches = $state<Batch[]>([])
  let summary = $state<ImportSummary | null>(null)
  let error = $state('')
  let busy = $state(false)
  let showDuplicates = $state(false)
  let nextBlockId = 1

  let newSourceName = $state('')
  let newSourceKind = $state<'card' | 'checking'>('card')

  $effect(() => {
    void app.version
    refresh()
  })

  async function refresh() {
    try {
      const [s, b, sav] = await Promise.all([api.listImportSources(), api.listImportBatches(), api.listSavings()])
      sources = s
      batches = b
      savings = sav.map((x) => x.account)
      if (blocks.length === 0) addBlock()
    } catch (e) {
      error = errorText(e)
    }
  }

  function addBlock() {
    const used = new Set(blocks.map((b) => b.sourceId))
    const next = sources.find((s) => !used.has(s.id))?.id ?? null
    blocks.push({ id: nextBlockId++, sourceId: next, text: '', preview: null, flip: null, error: '' })
  }

  function removeBlock(id: number) {
    blocks = blocks.filter((b) => b.id !== id)
    rows = rows.filter((r) => r.blockId !== id)
    if (blocks.length === 0) addBlock()
  }

  async function addSource() {
    try {
      const s = await api.addImportSource(newSourceName, newSourceKind)
      sources = [...sources, s].sort((a, b) => a.name.localeCompare(b.name))
      newSourceName = ''
      const empty = blocks.find((b) => b.sourceId === null)
      if (empty) empty.sourceId = s.id
      error = ''
    } catch (e) {
      error = errorText(e)
    }
  }

  async function openFile(block: Block, e: Event) {
    const file = (e.currentTarget as HTMLInputElement).files?.[0]
    if (file) block.text = await file.text()
    ;(e.currentTarget as HTMLInputElement).value = ''
  }

  async function ensureOptions(years: number[]) {
    for (const y of years) {
      if (!optionsByYear[y]) optionsByYear[y] = optionsFrom(await api.overview(y), savings)
    }
  }

  async function previewBlock(block: Block) {
    if (block.sourceId === null) return (block.error = 'Choose which card or account this is')
    if (!block.text.trim()) return (block.error = 'Paste some transactions first')
    busy = true
    try {
      const p = await api.importPreview(block.sourceId, block.text, block.flip)
      block.preview = p
      block.error = p.rows.length === 0 ? "Couldn't find any transactions in that text." : ''
      await ensureOptions([...new Set(p.rows.map((r) => Number(r.date.slice(0, 4))))])
      const sourceId = block.sourceId
      rows = [
        ...rows.filter((r) => r.blockId !== block.id),
        ...p.rows.map((r, i): Row => ({
          key: `${block.id}:${i}`,
          blockId: block.id,
          sourceId,
          row: r,
          action:
            r.status.kind === 'duplicate' ? 'skip' : r.status.kind === 'replaces_pending' ? 'replace_pending' : 'import',
          tag: r.suggestion?.tag ?? null,
        })),
      ]
      summary = null
    } catch (e) {
      block.error = errorText(e)
    } finally {
      busy = false
    }
  }

  function flipSigns(block: Block) {
    if (!block.preview) return
    block.flip = !block.preview.purchases_positive
    previewBlock(block)
  }

  // Attention first: problems, possible duplicates, guesses; confident rows after; duplicates hidden
  function priority(r: Row): number {
    if (r.row.problem && r.action !== 'skip') return 0
    if (r.row.status.kind === 'possible_duplicate') return 1
    if (r.row.suggestion?.auto && sameTag(r.tag, r.row.suggestion.tag)) return 2
    if (r.row.status.kind === 'replaces_pending') return 3
    if (r.row.status.kind === 'duplicate') return 9
    return 4
  }
  const sorted = $derived([...rows].sort((a, b) => priority(a) - priority(b) || a.row.date.localeCompare(b.row.date)))
  const visible = $derived(showDuplicates ? sorted : sorted.filter((r) => r.row.status.kind !== 'duplicate'))
  const duplicates = $derived(rows.filter((r) => r.row.status.kind === 'duplicate').length)
  const toImport = $derived(rows.filter((r) => r.action !== 'skip'))
  const missingTags = $derived(toImport.filter((r) => !r.tag).length)
  const needsAttention = $derived(rows.filter((r) => priority(r) <= 2).length)

  function sourceName(id: number) {
    return sources.find((s) => s.id === id)?.name ?? ''
  }

  async function commit() {
    if (toImport.length === 0 && rows.length === 0) return
    busy = true
    try {
      const commitRows: CommitRow[] = rows.map((r) => {
        const changedTag = !!r.tag && !sameTag(r.tag, r.row.suggestion?.tag)
        return {
          source_id: r.sourceId,
          date: r.row.date,
          description: r.row.description,
          amount_cents: r.row.amount_cents,
          pending: r.row.pending,
          action: r.action,
          existing_id: r.action === 'replace_pending' && r.row.status.kind === 'replaces_pending' ? r.row.status.existing_id : null,
          tag: r.action === 'skip' ? null : r.tag,
          auto: !changedTag && !!r.row.suggestion?.auto,
          learn: changedTag,
        }
      })
      const formats = blocks
        .filter((b) => b.preview && b.sourceId !== null)
        .map((b) => ({ source_id: b.sourceId!, purchases_positive: b.preview!.purchases_positive }))
      summary = await api.importCommit(commitRows, formats)
      rows = []
      blocks = []
      optionsByYear = {}
      error = ''
      changed()
    } catch (e) {
      error = errorText(e)
    } finally {
      busy = false
    }
  }

  async function undo(batchId: number) {
    if (!confirm('Undo this import? Its transactions are removed and anything it replaced comes back.')) return
    busy = true
    try {
      await api.undoImport(batchId)
      if (summary?.batch_id === batchId) summary = null
      error = ''
      changed()
    } catch (e) {
      error = errorText(e)
    } finally {
      busy = false
    }
  }

  function summaryText(s: ImportSummary): string {
    const parts = [`${s.imported} imported`]
    if (s.skipped) parts.push(`${s.skipped} already imported, skipped`)
    if (s.pending_replaced)
      parts.push(`${s.pending_replaced} pending → posted${s.pending_amount_changed ? ` (${s.pending_amount_changed} amount changed)` : ''}`)
    if (s.stale_pending_removed) parts.push(`${s.stale_pending_removed} stale pending removed`)
    return parts.join(' · ')
  }

  function onKeydown(e: KeyboardEvent) {
    if ((e.metaKey || e.ctrlKey) && e.key === 'Enter' && rows.length > 0 && missingTags === 0 && !busy) {
      e.preventDefault()
      commit()
    }
  }
</script>

<svelte:window onkeydown={onKeydown} />

<div class="row" style="margin-bottom: 14px">
  <h1>Import</h1>
  <span class="muted">Paste from each card or account's website, review, and import.</span>
</div>

{#if error}<p class="notice error" style="margin-bottom: 12px">{error}</p>{/if}

{#if summary}
  <div class="notice ok row" style="margin-bottom: 14px">
    <span>Done: {summaryText(summary)}</span>
    <span class="spacer"></span>
    <button onclick={() => undo(summary!.batch_id)} disabled={busy}>Undo</button>
    <button class="primary" onclick={() => go('dashboard')}>See where things stand</button>
  </div>
{/if}

{#if sources.length === 0}
  <div class="section">
    <div class="section-head"><h2>First, name your cards and accounts</h2></div>
    <div class="section-body stack">
      <p class="muted">
        Each source remembers how its bank formats transactions, so you only confirm it once. Use labels like "Card A"
        or "Checking"; account numbers aren't needed.
      </p>
    </div>
  </div>
{/if}

<div class="section">
  <div class="section-head">
    <h2>Sources</h2>
    <span class="spacer"></span>
    <input placeholder="New source name" bind:value={newSourceName} style="width: 160px" />
    <select bind:value={newSourceKind}>
      <option value="card">Credit card</option>
      <option value="checking">Checking</option>
    </select>
    <button disabled={!newSourceName.trim()} onclick={addSource}>Add source</button>
  </div>
  {#each blocks as block (block.id)}
    <div class="section-body stack block">
      <div class="row">
        <select bind:value={block.sourceId}>
          <option value={null}>Which card or account?</option>
          {#each sources as s (s.id)}<option value={s.id}>{s.name}</option>{/each}
        </select>
        <label class="link-like">
          <input type="file" accept=".csv,.tsv,.txt,text/csv,text/plain" onchange={(e) => openFile(block, e)} hidden />
          <span class="faint">or open a CSV file…</span>
        </label>
        <span class="spacer"></span>
        {#if block.preview}
          <span class="muted small">
            {block.preview.rows.length} found · {block.preview.skipped_lines} other lines skipped ·
            purchases shown as {block.preview.purchases_positive ? 'positive' : 'negative'}{block.preview.sign_detected ? ' (detected)' : ''}
          </span>
          <button class="link" onclick={() => flipSigns(block)} title="Use if spending shows up as money in">Flip signs</button>
        {/if}
        {#if blocks.length > 1}<button class="link danger" onclick={() => removeBlock(block.id)}>Remove</button>{/if}
      </div>
      <textarea
        rows={block.preview ? 3 : 8}
        placeholder="Paste transactions here: select the rows on the bank's website, copy, and paste. Pending items are fine."
        bind:value={block.text}
      ></textarea>
      {#if block.error}<p class="notice error">{block.error}</p>{/if}
      <div class="row">
        <button class="primary" disabled={busy} onclick={() => previewBlock(block)}>{block.preview ? 'Re-read' : 'Read transactions'}</button>
      </div>
    </div>
  {/each}
  <div class="section-body"><button onclick={addBlock}>+ Another source</button></div>
</div>

{#if rows.length > 0}
  <div class="section">
    <div class="section-head">
      <h2>Review</h2>
      <span class="muted">
        {toImport.length} to import{#if needsAttention}{` · ${needsAttention} worth a look`}{/if}
      </span>
      <span class="spacer"></span>
      {#if duplicates > 0}
        <label class="small muted"><input type="checkbox" bind:checked={showDuplicates} /> Show {duplicates} already imported</label>
      {/if}
      <button class="primary" disabled={busy || missingTags > 0} onclick={commit} title="⌘↩">
        Import {toImport.length} transaction{toImport.length === 1 ? '' : 's'}
      </button>
    </div>
    {#if missingTags > 0}<p class="notice" style="margin: 10px 14px 0">{missingTags} row{missingTags === 1 ? '' : 's'} can't be imported yet. Skip them or create the missing plan.</p>{/if}
    <table>
      <thead>
        <tr><th>Date</th><th>Description</th><th class="num">Amount</th><th>Tag</th><th>Note</th><th>Source</th></tr>
      </thead>
      <tbody>
        {#each visible as r (r.key)}
          {@const s = r.row.status}
          {@const year = Number(r.row.date.slice(0, 4))}
          <tr class:skipped={r.action === 'skip'}>
            <td class="nowrap">{shortDate(r.row.date, app.year)}</td>
            <td>
              <span title={r.row.description}>{r.row.display_name}</span>
              {#if r.row.pending}<span class="badge pending">pending</span>{/if}
            </td>
            <td class="num" class:text-green={r.row.amount_cents > 0}>{money(r.row.amount_cents)}</td>
            <td>
              {#if r.action !== 'skip' && optionsByYear[year]}
                <TagSelect tag={r.tag} options={optionsByYear[year]} onchange={(t) => (r.tag = t)} />
                {#if r.row.suggestion?.auto && sameTag(r.tag, r.row.suggestion.tag)}<span class="badge auto">auto</span>{/if}
              {:else if r.action === 'skip'}
                <span class="faint">skipped</span>
              {/if}
            </td>
            <td class="small">
              {#if r.row.problem && r.action !== 'skip'}
                <span class="text-red">{r.row.problem}</span>
                <button class="link" onclick={() => (r.action = 'skip')}>Skip</button>
              {:else if s.kind === 'possible_duplicate' && r.row.existing}
                <span>Same as {r.row.existing.display_name} {money(r.row.existing.amount_cents)} on {shortDate(r.row.existing.date)}?</span>
                <button class="link" class:chosen={r.action === 'skip'} onclick={() => (r.action = 'skip')}>Yes</button>
                <button class="link" class:chosen={r.action === 'import'} onclick={() => (r.action = 'import')}>No</button>
              {:else if s.kind === 'replaces_pending' && r.row.existing}
                <span class="muted">Posts pending {money(r.row.existing.amount_cents)}{s.amount_changed ? ' (amount changed)' : ''}</span>
              {:else if s.kind === 'duplicate'}
                <span class="faint">Already imported</span>
              {:else if r.row.suggestion?.reason === 'learned'}
                <span class="faint">Learned</span>
              {/if}
            </td>
            <td class="muted small">{sourceName(r.sourceId)}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
{/if}

{#if batches.length > 0}
  <div class="section">
    <div class="section-head"><h2>Recent imports</h2></div>
    <table>
      <tbody>
        {#each batches.slice(0, 8) as b (b.id)}
          <tr>
            <td class="nowrap muted" style="width: 160px">{b.created_at} UTC</td>
            <td>{summaryText(b.summary)}</td>
            <td class="num">
              {#if b.undone_at}<span class="faint">undone</span>{:else}<button class="link" disabled={busy} onclick={() => undo(b.id)}>Undo</button>{/if}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
{/if}

<style>
  .block + .block {
    border-top: 1px solid var(--border);
  }
  tr.skipped td {
    color: var(--text-3);
  }
  .chosen {
    font-weight: 600;
    text-decoration: underline;
  }
  .link-like {
    cursor: pointer;
  }
</style>

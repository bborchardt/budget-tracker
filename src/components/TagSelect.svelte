<script lang="ts">
  // One select for any tag: spending categories, income sources, or a transfer.
  // Native selects support type-to-find, which keeps tagging fast from the keyboard.
  import type { Tag } from '../lib/types'
  import { tagFromKey, tagKey, type TagOptions } from '../lib/tags'

  let {
    tag,
    options,
    onchange,
    disabled = false,
  }: { tag: Tag | null; options: TagOptions; onchange: (tag: Tag | null) => void; disabled?: boolean } = $props()
</script>

<select
  value={tagKey(tag)}
  {disabled}
  onchange={(e) => onchange(tagFromKey((e.currentTarget as HTMLSelectElement).value))}
>
  {#if !tag}<option value="">Choose…</option>{/if}
  <optgroup label="Spending">
    {#each options.categories as c (c.id)}
      <option value={`e:${c.id}`}>{c.name}</option>
    {/each}
  </optgroup>
  <optgroup label="Income">
    {#each options.income as s (s.id)}
      <option value={`i:${s.id}`}>{s.name}</option>
    {/each}
  </optgroup>
  <optgroup label="Transfer (not spending)">
    <option value="t:">Transfer</option>
    {#each options.savings as a (a.id)}
      <option value={`t:${a.id}`}>Transfer: {a.name}</option>
    {/each}
  </optgroup>
</select>

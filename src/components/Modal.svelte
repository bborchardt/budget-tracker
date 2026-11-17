<script lang="ts">
  import type { Snippet } from 'svelte'

  let { title, onclose, children }: { title: string; onclose: () => void; children: Snippet } = $props()

  function keydown(e: KeyboardEvent) {
    if (e.key === 'Escape') onclose()
  }
</script>

<svelte:window onkeydown={keydown} />

<div class="backdrop" role="presentation" onclick={(e) => e.target === e.currentTarget && onclose()}>
  <div class="modal" role="dialog" aria-modal="true" aria-label={title}>
    <div class="section-head">
      <h2>{title}</h2>
      <span class="spacer"></span>
      <button class="link" onclick={onclose} aria-label="Close">✕</button>
    </div>
    <div class="section-body">
      {@render children()}
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.35);
    display: flex;
    align-items: flex-start;
    justify-content: center;
    padding-top: 12vh;
    z-index: 50;
  }
  .modal {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 8px;
    width: min(520px, calc(100vw - 32px));
    max-height: 76vh;
    overflow: auto;
    box-shadow: 0 10px 30px rgba(0, 0, 0, 0.2);
  }
</style>

<script lang="ts">
  import type { Status } from '../lib/types'
  let { status }: { status: Status } = $props()
  const labels: Record<Status, string> = { green: 'On track', yellow: 'Ahead of pace', red: 'Over pace or over budget' }
</script>

<span class="dot {status}" title={labels[status]} aria-label={labels[status]}></span>

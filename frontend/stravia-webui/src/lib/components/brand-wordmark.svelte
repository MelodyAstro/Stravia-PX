<script lang="ts">

import { cn } from '#lib/utils.js'

interface Props {
  class?: string
}

let { class: className }: Props = $props()
</script>

<span class={cn('inline-flex shrink-0 align-middle font-structural font-semibold tracking-tight', className)} role="img" aria-label="Strinova">
  Strinova
</span>

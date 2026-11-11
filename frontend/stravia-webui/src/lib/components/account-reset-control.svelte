<script lang="ts">
import * as m from '#lib/paraglide/messages.js'
import { createQuery } from '@tanstack/svelte-query'
import { toast } from 'svelte-sonner'
import { admin } from '#lib/admin-client.js'
import { createUuid } from '#lib/uuid.js'
import { localizeBackendErrorMessage } from '#lib/backend-error.js'
import { Button } from '#lib/components/ui/button/index.js'
import * as AlertDialog from '#lib/components/ui/alert-dialog/index.js'

let { providerId, available, onrefresh }: { providerId: string; available: number | null; onrefresh: () => void } = $props()
let open = $state(false)
let busy = $state(false)
let requestId = $state<string | null>(null)
const state = createQuery(() => ({ queryKey: ['account-reset-state', providerId], queryFn: () => admin.allowances.resetState(providerId) }))

async function redeem(): Promise<void> {
  if (busy) return
  busy = true
  requestId = state.data?.pending ? state.data.request_id : requestId ?? createUuid()
  try {
    if (!requestId) throw new Error('Missing reset request identifier')
    const result = await admin.allowances.redeemReset(providerId, requestId)
    toast.message(m.account_reset_done({ result: result.code }))
    requestId = null
    await state.refetch()
    onrefresh()
    open = false
  } catch (error) {
    toast.error(localizeBackendErrorMessage(error))
    await state.refetch()
  } finally {
    busy = false
  }
}
</script>

<div class="flex flex-wrap items-center gap-2 py-2">
  <Button type="button" variant="outline" size="sm" disabled={busy || state.isPending || state.isError || (!state.data?.pending && (available === null || available < 1))} onclick={() => open = true}>{m.account_reset_use()}</Button>
  {#if state.data?.pending}<span class="text-xs text-muted-foreground" role="status">{m.account_reset_pending()}</span>
  {:else if available === null}<span class="text-xs text-muted-foreground">{m.account_reset_unknown()}</span>{/if}
</div>
<AlertDialog.Root bind:open>
  <AlertDialog.Content>
    <AlertDialog.Header>
      <AlertDialog.Title>{m.account_reset_confirm()}</AlertDialog.Title>
      <AlertDialog.Description>{m.account_reset_warning()}</AlertDialog.Description>
    </AlertDialog.Header>
    <AlertDialog.Footer>
      <AlertDialog.Cancel disabled={busy}>{m.account_reset_cancel()}</AlertDialog.Cancel>
      <Button disabled={busy} onclick={() => void redeem()}>{m.account_reset_use()}</Button>
    </AlertDialog.Footer>
  </AlertDialog.Content>
</AlertDialog.Root>

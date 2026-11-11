<script lang="ts">
import * as m from '#lib/paraglide/messages.js'
import { createQuery } from '@tanstack/svelte-query'
import { toast } from 'svelte-sonner'
import { admin } from '#lib/admin-client.js'
import { createUuid } from '#lib/uuid.js'
import { localizeBackendErrorMessage } from '#lib/backend-error.js'
import { Button } from '#lib/components/ui/button/index.js'
import * as AlertDialog from '#lib/components/ui/alert-dialog/index.js'

let { providerId, available, onrefresh }: { providerId: string; available: number | null; onrefresh: () => void } =
  $props()
let open = $state(false)
let busy = $state(false)
let requestId = $state<string | null>(null)
const resetQuery = createQuery(() => ({
  queryKey: ['account-reset-state', providerId],
  queryFn: () => admin.allowances.resetState(providerId),
}))

function resultLabel(code: string): string {
  switch (code) {
    case 'reset':
      return m.account_reset_result_reset()
    case 'already_redeemed':
    case 'already_used':
      return m.account_reset_result_already_used()
    case 'nothing_to_reset':
    case 'not_limited':
      return m.account_reset_result_not_limited()
    case 'no_credit':
      return m.account_reset_result_no_credit()
    case 'cooldown':
      return m.account_reset_result_cooldown()
    default:
      return m.account_reset_result_unavailable()
  }
}

async function redeem(): Promise<void> {
  if (busy) return
  busy = true
  requestId = resetQuery.data?.pending ? resetQuery.data.request_id : (requestId ?? createUuid())
  try {
    if (!requestId) throw new Error('Missing reset request identifier')
    const result = await admin.allowances.redeemReset(providerId, requestId)
    toast.message(m.account_reset_done({ result: resultLabel(result.code) }))
    requestId = null
    await resetQuery.refetch()
    onrefresh()
    open = false
  } catch (error) {
    toast.error(localizeBackendErrorMessage(error))
    await resetQuery.refetch()
  } finally {
    busy = false
  }
}
</script>

<div class="flex flex-wrap items-center gap-2 py-2">
  <Button
    type="button"
    variant="outline"
    size="sm"
    disabled={busy ||
      resetQuery.isPending ||
      resetQuery.isError ||
      (!resetQuery.data?.pending && (available === null || available < 1))}
    onclick={() => (open = true)}>{m.account_reset_use()}</Button>
  {#if resetQuery.data?.pending}<span class="text-xs text-muted-foreground" role="status"
      >{m.account_reset_pending()}</span>
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

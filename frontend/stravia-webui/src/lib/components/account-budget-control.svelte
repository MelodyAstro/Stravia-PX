<script lang="ts">
import { createQuery } from '@tanstack/svelte-query'
import { toast } from 'svelte-sonner'
import * as m from '#lib/paraglide/messages.js'
import { admin } from '#lib/admin-client.js'
import { localizeBackendErrorMessage } from '#lib/backend-error.js'
import { Button } from '#lib/components/ui/button/index.js'
import { Input } from '#lib/components/ui/input/index.js'
import { Label } from '#lib/components/ui/label/index.js'
import { Switch } from '#lib/components/ui/switch/index.js'

let { providerId }: { providerId: string } = $props()
let editing = $state(false)
let enabled = $state(false)
let limit = $state('0')
let revision = $state(0)
let busy = $state(false)
const budget = createQuery(() => ({
  queryKey: ['account-budget', providerId],
  queryFn: () => admin.allowances.budget(providerId),
}))

async function edit(): Promise<void> {
  const result = await budget.refetch()
  if (!result.data) return
  enabled = result.data.limit !== null
  limit = result.data.limit ?? '0'
  revision = result.data.revision
  editing = true
}

async function save(event: SubmitEvent): Promise<void> {
  event.preventDefault()
  if (busy) return
  busy = true
  try {
    await admin.allowances.saveBudget(providerId, enabled ? limit : null, revision)
    await budget.refetch()
    editing = false
    toast.success(m.account_budget_saved())
  } catch (error) {
    toast.error(localizeBackendErrorMessage(error))
  } finally {
    busy = false
  }
}
</script>

<section class="my-3 space-y-3 rounded-xl border p-4" aria-label={m.account_budget_title()}>
  <div class="flex flex-wrap items-start justify-between gap-3">
    <div class="min-w-0 space-y-1">
      <p class="text-sm font-medium">{m.account_budget_title()}</p>
      {#if budget.data}
        <p class="font-technical text-sm" data-testid="account-budget-summary">
          {m.account_budget_summary({
            spent: budget.data.spent,
            reserved: budget.data.reserved,
            unit: budget.data.unit,
          })}
        </p>
        <p class="text-xs text-muted-foreground">
          {budget.data.limit === null
            ? m.account_budget_disabled()
            : m.account_budget_limit_value({ value: budget.data.limit, unit: budget.data.unit })}
        </p>
      {:else if budget.isError}
        <p class="text-sm text-destructive" role="status">{m.account_budget_unavailable()}</p>
      {/if}
    </div>
    <Button type="button" variant="outline" size="sm" disabled={busy || budget.isPending} onclick={() => void edit()}
      >{m.account_budget_edit()}</Button>
  </div>
  <p class="text-xs leading-relaxed text-muted-foreground">{m.account_budget_scope()}</p>
  {#if budget.data && budget.data.unpriced_requests > 0}
    <p class="text-xs text-muted-foreground" role="status">{m.account_budget_unpriced({ count: budget.data.unpriced_requests })}</p>
  {/if}
  {#if budget.data && budget.data.uncertain_spend !== '0'}
    <p class="text-xs text-muted-foreground" role="status">
      {m.account_budget_uncertain({ value: budget.data.uncertain_spend, unit: budget.data.unit })}
    </p>
  {/if}
  {#if editing && budget.data}
    <form class="space-y-3 border-t pt-3" onsubmit={save}>
      <div class="flex items-center justify-between gap-3">
        <Label for={`account-budget-enabled-${providerId}`}>{m.account_budget_enable()}</Label>
        <Switch id={`account-budget-enabled-${providerId}`} bind:checked={enabled} disabled={busy} />
      </div>
      {#if enabled}
        <div class="space-y-2">
          <Label for={`account-budget-limit-${providerId}`}
            >{m.account_budget_limit_label({ unit: budget.data.unit })}</Label>
          <Input
            id={`account-budget-limit-${providerId}`}
            bind:value={limit}
            inputmode="decimal"
            required
            disabled={busy} />
        </div>
      {/if}
      <p class="text-xs leading-relaxed text-muted-foreground">
        {m.account_budget_conservative({ date: budget.data.rate_date })}
      </p>
      <div class="flex flex-wrap gap-2">
        <Button type="submit" size="sm" disabled={busy}>{m.account_budget_save()}</Button>
        <Button type="button" variant="ghost" size="sm" disabled={busy} onclick={() => (editing = false)}
          >{m.account_reset_cancel()}</Button>
      </div>
    </form>
  {/if}
</section>

<script lang="ts">
import * as m from '#lib/paraglide/messages.js'
import { createQuery, useQueryClient } from '@tanstack/svelte-query'
import { toast } from 'svelte-sonner'
import { admin } from '#lib/admin-client.js'
import { localizeBackendErrorMessage } from '#lib/backend-error.js'
import { DraftBaseline, DraftGuard } from '#lib/draft-guard.svelte.js'
import type { RequestModelMapping, RequestModelsView } from '#lib/types/api-key.js'
import DraftDiscardDialog from '#lib/components/draft-discard-dialog.svelte'
import RequestFailure from '#lib/components/request-failure.svelte'
import SettingRow from '#lib/components/setting-row.svelte'
import { Button } from '#lib/components/ui/button/index.js'
import * as Field from '#lib/components/ui/field/index.js'
import { Input } from '#lib/components/ui/input/index.js'
import * as Select from '#lib/components/ui/select/index.js'
import { Spinner } from '#lib/components/ui/spinner/index.js'
import { Switch } from '#lib/components/ui/switch/index.js'

let { apiKeyId }: { apiKeyId: string } = $props()
const client = useQueryClient()
const query = createQuery(() => ({
  queryKey: ['request-models', apiKeyId],
  queryFn: () => admin.apiKeys.requestModels(apiKeyId),
  refetchInterval: (current) => (current.state.data?.capture.active ? 2000 : false),
}))
let draft = $state({ enabled: false, mappings: [] as (RequestModelMapping & { rowId: string })[] })
let loadedId = $state('')
let saving = $state(false)
let capturing = $state(false)
const baseline = new DraftBaseline(() => draft)
const guard = new DraftGuard({ dirty: () => baseline.dirty, busy: () => saving })

$effect(() => {
  if (query.data && loadedId !== apiKeyId) {
    draft = {
      enabled: query.data.config.enabled,
      mappings: query.data.config.mappings.map((mapping) => ({ ...mapping, rowId: crypto.randomUUID() })),
    }
    loadedId = apiKeyId
    baseline.reset()
  }
})

function addMapping(requestModel = '', target = ''): void {
  if (requestModel && draft.mappings.some((mapping) => mapping.request_model === requestModel)) return
  draft.mappings.push({ request_model: requestModel, target_model_id: target, rowId: crypto.randomUUID() })
}

function updateView(view: RequestModelsView): void {
  client.setQueryData(['request-models', apiKeyId], view)
}

async function save(): Promise<void> {
  if (saving) return
  saving = true
  try {
    const view = await admin.apiKeys.saveRequestModels(apiKeyId, {
      enabled: draft.enabled,
      mappings: draft.mappings.map(({ request_model, target_model_id }) => ({ request_model, target_model_id })),
    })
    updateView(view)
    baseline.reset()
    toast.success(m.request_models_saved())
  } catch (error) {
    toast.error(localizeBackendErrorMessage(error))
  } finally {
    saving = false
  }
}

async function capture(enabled: boolean | null): Promise<void> {
  if (capturing) return
  capturing = true
  try {
    updateView(
      enabled === null
        ? await admin.apiKeys.clearRequestModelCapture(apiKeyId)
        : await admin.apiKeys.captureRequestModels(apiKeyId, enabled),
    )
  } catch (error) {
    toast.error(localizeBackendErrorMessage(error))
  } finally {
    capturing = false
  }
}
</script>

<section id="request-models" aria-labelledby="request-models-title" class="min-w-0 scroll-mt-6 border-b pb-6">
  <h2 id="request-models-title" class="mb-4 text-base font-semibold">{m.request_models_title()}</h2>
  {#if query.isPending}
    <div role="status"><Spinner />{m.common_settings_loading()}</div>
  {:else if query.isError}
    <RequestFailure
      title={m.request_models_load_failed()}
      message={localizeBackendErrorMessage(query.error)}
      retry={() => query.refetch()}
      retrying={query.isFetching} />
  {:else if query.data}
    <div class="flex flex-col gap-4">
      <SettingRow title={m.request_models_enable()} hint={m.request_models_scope_help()} layout="inline" class="py-0">
        {#snippet children({ titleId })}<Switch
            aria-labelledby={titleId}
            bind:checked={draft.enabled}
            disabled={saving} />{/snippet}
      </SettingRow>
      {#each draft.mappings as mapping (mapping.rowId)}
        <Field.Group class="border-t pt-4">
          <Field.Field>
            <Field.Label for={'request-name-' + mapping.rowId}>{m.request_models_client_name()}</Field.Label>
            <Input
              id={'request-name-' + mapping.rowId}
              bind:value={mapping.request_model}
              disabled={saving}
              placeholder="gpt-6-luna"
              class="font-technical" />
          </Field.Field>
          <Field.Field>
            <Field.Label for={'request-target-' + mapping.rowId}>{m.request_models_target()}</Field.Label>
            <Select.Root type="single" bind:value={mapping.target_model_id} disabled={saving}>
              <Select.Trigger id={'request-target-' + mapping.rowId} class="w-full">
                {query.data.eligible_models.find((model) => model.id === mapping.target_model_id)?.model_id ??
                  (mapping.target_model_id || m.request_models_choose_target())}
              </Select.Trigger>
              <Select.Content>
                {#each query.data.eligible_models as model (model.id)}<Select.Item
                    value={model.id}
                    label={model.model_id}>{model.model_id}</Select.Item
                  >{/each}
              </Select.Content>
            </Select.Root>
          </Field.Field>
          <div>
            <Button
              type="button"
              variant="outline"
              disabled={saving}
              onclick={() => (draft.mappings = draft.mappings.filter((row) => row.rowId !== mapping.rowId))}
              >{m.common_remove()}</Button>
          </div>
        </Field.Group>
      {/each}
      <div class="flex flex-wrap gap-2">
        <Button
          type="button"
          variant="outline"
          disabled={saving || draft.mappings.length >= 128}
          onclick={() => addMapping()}>{m.request_models_add()}</Button>
        <Button type="button" disabled={saving || !baseline.dirty} onclick={() => void save()}
          >{#if saving}<Spinner data-icon="inline-start" />{/if}{m.request_models_save()}</Button>
      </div>

      <div class="flex flex-col gap-3 border-t pt-4">
        <h3 class="font-medium">{m.request_models_recognition()}</h3>
        <p class="text-sm text-muted-foreground">{m.request_models_capture_privacy()}</p>
        <div class="flex flex-wrap items-center gap-2">
          <Button
            type="button"
            variant="outline"
            disabled={capturing || query.data.capture.active}
            onclick={() => void capture(true)}>{m.request_models_start()}</Button>
          {#if query.data.capture.active}<Button
              type="button"
              variant="outline"
              disabled={capturing}
              onclick={() => void capture(false)}>{m.request_models_stop()}</Button
            >{/if}
          {#if query.data.capture.samples.length > 0 || query.data.capture.active}<Button
              type="button"
              variant="outline"
              disabled={capturing}
              onclick={() => void capture(null)}>{m.request_models_clear()}</Button
            >{/if}
          <span class="text-sm text-muted-foreground" role="status"
            >{query.data.capture.active ? m.request_models_listening() : m.request_models_stopped()}</span>
        </div>
        {#if query.data.capture.samples.length === 0}
          <p class="text-sm text-muted-foreground">{m.request_models_no_samples()}</p>
        {:else}
          <ul class="divide-y">
            {#each query.data.capture.samples as sample, index (index)}
              <li class="flex flex-wrap justify-between gap-2 py-2">
                <div class="min-w-0">
                  <div class="break-all font-technical">{sample.request_model}</div>
                  <div class="break-all text-xs text-muted-foreground">
                    {sample.protocol} · {sample.transport} · {m.request_models_sample_counts({
                      messages: sample.input_items,
                      tools: sample.tools,
                      count: sample.count,
                    })}
                  </div>
                </div>
                <Button
                  type="button"
                  variant="outline"
                  disabled={saving ||
                    draft.mappings.length >= 128 ||
                    draft.mappings.some((row) => row.request_model === sample.request_model)}
                  onclick={() =>
                    addMapping(
                      sample.request_model,
                      query.data?.suggestions.find((suggestion) => suggestion.request_model === sample.request_model)
                        ?.target_model_id ?? '',
                    )}>{m.request_models_use_sample()}</Button>
              </li>
            {/each}
          </ul>
          <p class="text-sm text-muted-foreground">{m.request_models_suggestion_help()}</p>
        {/if}
        {#if query.data.capture.dropped_samples > 0}<p class="text-sm text-muted-foreground" role="status">
            {m.request_models_dropped({ count: query.data.capture.dropped_samples })}
          </p>{/if}
      </div>
    </div>
  {/if}
</section>
<DraftDiscardDialog {guard} />

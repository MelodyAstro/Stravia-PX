<script lang="ts">
import * as m from '#lib/paraglide/messages.js'
import { createQuery, useQueryClient } from '@tanstack/svelte-query'
import PackageOpenIcon from '@lucide/svelte/icons/package-open'
import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw'
import UploadIcon from '@lucide/svelte/icons/upload'
import { toast } from 'svelte-sonner'

import { admin } from '#lib/admin-client.js'
import { localizeBackendErrorMessage } from '#lib/backend-error.js'
import { DraftBaseline, DraftGuard } from '#lib/draft-guard.svelte.js'
import type { PlatformPluginSummary } from '#lib/types.js'
import DraftDiscardDialog from '#lib/components/draft-discard-dialog.svelte'
import PageHeader from '#lib/components/page-header.svelte'
import RequestFailure from '#lib/components/request-failure.svelte'
import StatusIndicator from '#lib/components/status-indicator.svelte'
import TechnicalValue from '#lib/components/technical-value.svelte'
import * as Alert from '#lib/components/ui/alert/index.js'
import * as AlertDialog from '#lib/components/ui/alert-dialog/index.js'
import { Badge } from '#lib/components/ui/badge/index.js'
import { Button } from '#lib/components/ui/button/index.js'
import * as Empty from '#lib/components/ui/empty/index.js'
import * as Field from '#lib/components/ui/field/index.js'
import * as Sheet from '#lib/components/ui/sheet/index.js'
import { Skeleton } from '#lib/components/ui/skeleton/index.js'
import { Spinner } from '#lib/components/ui/spinner/index.js'
import { Switch } from '#lib/components/ui/switch/index.js'
import { Textarea } from '#lib/components/ui/textarea/index.js'

const MAX_PLUGIN_BYTES = 64 * 1024 * 1024
const queryClient = useQueryClient()
const pluginsQuery = createQuery(() => ({ queryKey: ['platform-plugins'], queryFn: admin.platformPlugins.list }))
const plugins = $derived(pluginsQuery.data ?? [])
const listEmpty = $derived(!pluginsQuery.isPending && !pluginsQuery.error && plugins.length === 0)

let fileInput = $state<HTMLInputElement | null>(null)
let importing = $state(false)
let importError = $state('')
let updateId = $state<string>()
let savingId = $state<string>()
let actionError = $state<{ id: string; message: string }>()
let editTarget = $state<PlatformPluginSummary>()
let editorOpen = $state(false)
let configText = $state('')
let configError = $state('')
let savingConfig = $state(false)
let uninstallTarget = $state<PlatformPluginSummary>()
let uninstalling = $state(false)
let uninstallError = $state('')
const baseline = new DraftBaseline(() => configText)
const guard = new DraftGuard({ dirty: () => editorOpen && baseline.dirty, busy: () => savingConfig })

function pickFile(id?: string): void {
  updateId = id
  if (fileInput) {
    fileInput.value = ''
    fileInput.click()
  }
}

async function installFile(event: Event): Promise<void> {
  const file = (event.currentTarget as HTMLInputElement).files?.[0]
  if (!file || importing) return
  importError = ''
  if (file.size === 0 || file.size > MAX_PLUGIN_BYTES) {
    importError = m.platform_plugins_file_size()
    return
  }
  importing = true
  try {
    const plugin = await admin.platformPlugins.install(file)
    queryClient.setQueryData<PlatformPluginSummary[]>(['platform-plugins'], (current = []) =>
      [...current.filter((item) => item.manifest.id !== plugin.manifest.id), plugin].sort((a, b) =>
        a.manifest.id.localeCompare(b.manifest.id),
      ),
    )
    toast.success(m.platform_plugins_installed())
    openEditor(plugin)
  } catch (error) {
    importError = localizeBackendErrorMessage(error)
  } finally {
    importing = false
    updateId = undefined
  }
}

async function setEnabled(plugin: PlatformPluginSummary, enabled: boolean): Promise<void> {
  if (savingId) return
  savingId = plugin.manifest.id
  actionError = undefined
  try {
    const saved = await admin.platformPlugins.settings(plugin.manifest.id, { enabled })
    replacePlugin(saved)
  } catch (error) {
    actionError = { id: plugin.manifest.id, message: localizeBackendErrorMessage(error) }
  } finally {
    savingId = undefined
  }
}

function replacePlugin(plugin: PlatformPluginSummary): void {
  queryClient.setQueryData<PlatformPluginSummary[]>(['platform-plugins'], (current = []) =>
    current.map((item) => (item.manifest.id === plugin.manifest.id ? plugin : item)),
  )
  if (editTarget?.manifest.id === plugin.manifest.id) editTarget = plugin
}

function openEditor(plugin: PlatformPluginSummary): void {
  editTarget = plugin
  configText = JSON.stringify(plugin.config, null, 2)
  configError = ''
  baseline.reset()
  editorOpen = true
}

async function saveConfig(event: SubmitEvent): Promise<void> {
  event.preventDefault()
  if (!editTarget || savingConfig || !baseline.dirty) return
  configError = ''
  let config: unknown
  try {
    config = JSON.parse(configText)
  } catch {
    configError = m.platform_plugins_invalid_json()
    return
  }
  savingConfig = true
  try {
    const saved = await admin.platformPlugins.settings(editTarget.manifest.id, { config })
    replacePlugin(saved)
    configText = JSON.stringify(saved.config, null, 2)
    baseline.reset()
    editorOpen = false
    toast.success(m.platform_plugins_config_saved())
  } catch (error) {
    configError = localizeBackendErrorMessage(error)
  } finally {
    savingConfig = false
  }
}

function prepareUninstall(plugin: PlatformPluginSummary): void {
  uninstallTarget = plugin
  uninstallError = ''
}

async function uninstall(): Promise<void> {
  if (!uninstallTarget || uninstalling) return
  uninstalling = true
  uninstallError = ''
  const id = uninstallTarget.manifest.id
  try {
    await admin.platformPlugins.uninstall(id)
    queryClient.setQueryData<PlatformPluginSummary[]>(['platform-plugins'], (current = []) =>
      current.filter((plugin) => plugin.manifest.id !== id),
    )
    uninstallTarget = undefined
    toast.success(m.platform_plugins_uninstalled())
  } catch (error) {
    uninstallError = localizeBackendErrorMessage(error)
  } finally {
    uninstalling = false
  }
}

function hookLabel(hook: string): string {
  switch (hook) {
    case 'request':
      return m.platform_plugins_hook_request()
    case 'upstream_response':
      return m.platform_plugins_hook_upstream_response()
    case 'tool_result':
      return m.platform_plugins_hook_tool_result()
    case 'client_output':
      return m.platform_plugins_hook_client_output()
    default:
      return hook
  }
}
</script>

{#snippet pageActions()}
  <Button variant="outline" size="sm" onclick={() => void pluginsQuery.refetch()} disabled={pluginsQuery.isFetching}>
    <RefreshCwIcon data-icon="inline-start" />{m.platform_plugins_refresh()}
  </Button>
  <Button size="sm" onclick={() => pickFile()} disabled={importing || editorOpen} aria-busy={importing}>
    {#if importing}<Spinner data-icon="inline-start" />{:else}<UploadIcon data-icon="inline-start" />{/if}
    {m.platform_plugins_install()}
  </Button>
{/snippet}

<svelte:head><title>{m.platform_plugins_title()} · Stravia</title></svelte:head>

<div class="route-page platform-plugins-page">
  <PageHeader title={m.platform_plugins_title()} actions={listEmpty ? undefined : pageActions} />
  <input bind:this={fileInput} type="file" accept=".wasm,application/wasm" class="hidden" onchange={installFile} />

  {#if pluginsQuery.error}
    <RequestFailure
      title={m.platform_plugins_load_failed()}
      message={localizeBackendErrorMessage(pluginsQuery.error)}
      retry={() => pluginsQuery.refetch()}
      retrying={pluginsQuery.isFetching} />
  {/if}
  {#if importError}
    <RequestFailure title={m.platform_plugins_install_failed()} message={importError} />
  {/if}

  <section class="route-section platform-plugins-workspace" aria-label={m.platform_plugins_title()}>
    {#if listEmpty}
      <Empty.Root class="mx-auto my-auto w-full max-w-xl py-10">
        <Empty.Header>
          <Empty.Media variant="icon"><PackageOpenIcon /></Empty.Media>
          <Empty.Title>{m.platform_plugins_empty_title()}</Empty.Title>
          <Empty.Description>{m.platform_plugins_empty_description()}</Empty.Description>
        </Empty.Header>
        <Empty.Content>
          <Button onclick={() => pickFile()} disabled={importing} aria-busy={importing}>
            {#if importing}<Spinner data-icon="inline-start" />{:else}<UploadIcon data-icon="inline-start" />{/if}
            {m.platform_plugins_install()}
          </Button>
        </Empty.Content>
      </Empty.Root>
    {:else if pluginsQuery.isPending}
      <div class="divide-y border-y" aria-busy="true">
        {#each Array(3) as _, index (index)}
          <div class="flex flex-col gap-3 py-4">
            <Skeleton class="h-5 w-40" /><Skeleton class="h-4 w-64 max-w-full" />
          </div>
        {/each}
      </div>
    {:else if plugins.length > 0}
      <ul class="divide-y border-y">
        {#each plugins as plugin (plugin.manifest.id)}
          <li class="flex min-w-0 flex-col gap-3 py-4" data-plugin-id={plugin.manifest.id}>
            <div class="flex flex-wrap items-start justify-between gap-3">
              <div class="min-w-0 flex-1">
                <div class="flex flex-wrap items-center gap-x-3 gap-y-1">
                  <h2 class="font-medium break-words">{plugin.manifest.name}</h2>
                  <span class="font-technical text-xs tabular-nums text-muted-foreground"
                    >{plugin.manifest.version}</span>
                  <StatusIndicator
                    compact
                    tone={plugin.error ? 'error' : plugin.enabled ? 'healthy' : 'neutral'}
                    label={plugin.error
                      ? m.platform_plugins_unavailable()
                      : plugin.enabled
                        ? m.common_enabled_status()
                        : m.common_disabled_status()} />
                </div>
                <TechnicalValue value={plugin.manifest.id} copyable />
              </div>
              <div class="flex items-center gap-2">
                {#if savingId === plugin.manifest.id}<Spinner />{/if}
                <Switch
                  bind:checked={() => plugin.enabled, (checked: boolean) => void setEnabled(plugin, checked)}
                  disabled={Boolean(savingId) || importing || (Boolean(plugin.error) && !plugin.enabled)}
                  aria-label={m.platform_plugins_enable_named({ name: plugin.manifest.name })} />
              </div>
            </div>
            <div class="flex flex-wrap items-center justify-between gap-3">
              <div class="flex min-w-0 flex-wrap gap-1.5">
                {#each plugin.manifest.hooks as hook (hook)}<Badge variant="secondary">{hookLabel(hook)}</Badge>{/each}
                {#each plugin.manifest.tools as tool (tool.name)}<Badge variant="outline">{tool.name}</Badge>{/each}
              </div>
              <div class="flex flex-wrap gap-2">
                <Button variant="outline" size="sm" onclick={() => openEditor(plugin)} disabled={importing}
                  >{m.platform_plugins_configure()}</Button>
                <Button
                  variant="outline"
                  size="sm"
                  onclick={() => pickFile(plugin.manifest.id)}
                  disabled={importing || editorOpen}
                  aria-busy={importing && updateId === plugin.manifest.id}>{m.platform_plugins_update()}</Button>
                <Button variant="ghost" size="sm" onclick={() => prepareUninstall(plugin)} disabled={importing}
                  >{m.platform_plugins_uninstall()}</Button>
              </div>
            </div>
            {#if plugin.error}<RequestFailure message={plugin.error} />{/if}
            {#if actionError?.id === plugin.manifest.id}<RequestFailure message={actionError.message} />{/if}
          </li>
        {/each}
      </ul>
    {/if}
  </section>
</div>

<Sheet.Root bind:open={() => editorOpen, guard.openChange((next) => (editorOpen = next))}>
  <Sheet.Content
    class="route-overlay-content-md w-full! gap-0 overflow-hidden p-0 sm:max-w-[40rem]!"
    locked={savingConfig}
    closeLabel={m.common_close()}>
    <Sheet.Header class="border-b pr-12">
      <Sheet.Title>{editTarget?.manifest.name ?? m.platform_plugins_configure()}</Sheet.Title>
      <Sheet.Description>{m.platform_plugins_config_description()}</Sheet.Description>
    </Sheet.Header>
    {#if editTarget}
      <form class="route-overlay-form" onsubmit={saveConfig}>
        <div class="route-overlay-body flex flex-col gap-6">
          <dl class="grid grid-cols-[auto_minmax(0,1fr)] gap-x-4 gap-y-2">
            <dt class="text-muted-foreground">{m.platform_plugins_id()}</dt>
            <dd class="min-w-0"><TechnicalValue value={editTarget.manifest.id} copyable /></dd>
            <dt class="text-muted-foreground">{m.platform_plugins_version()}</dt>
            <dd class="font-technical text-xs">{editTarget.manifest.version}</dd>
            <dt class="text-muted-foreground">{m.platform_plugins_digest()}</dt>
            <dd class="min-w-0"><TechnicalValue value={editTarget.sha256} copyable /></dd>
          </dl>
          <section class="flex flex-col gap-3">
            <h2 class="font-structural text-base font-semibold">{m.platform_plugins_capabilities()}</h2>
            <div class="flex flex-wrap gap-1.5">
              {#each editTarget.manifest.hooks as hook (hook)}<Badge variant="secondary">{hookLabel(hook)}</Badge
                >{/each}
            </div>
            {#each editTarget.manifest.tools as tool (tool.name)}
              <div>
                <p class="font-technical text-xs">{tool.name}</p>
                <p class="mt-1 text-muted-foreground">{tool.description}</p>
              </div>
            {/each}
            {#if !editTarget.manifest.hooks.length && !editTarget.manifest.tools.length}<p
                class="text-muted-foreground">
                {m.platform_plugins_no_capabilities()}
              </p>{/if}
          </section>
          {#if !editTarget.enabled}
            <Alert.Root><Alert.Description>{m.platform_plugins_disabled_hint()}</Alert.Description></Alert.Root>
          {/if}
          <Field.Field>
            <Field.Label for="platform-plugin-config" hint={m.platform_plugins_config_help()}
              >{m.platform_plugins_config()}</Field.Label>
            <Textarea
              id="platform-plugin-config"
              class="min-h-48 font-mono text-xs"
              bind:value={configText}
              disabled={savingConfig}
              spellcheck={false}
              aria-invalid={Boolean(configError)} />
          </Field.Field>
          {#if editTarget.manifest.config_schema != null}
            <details>
              <summary class="cursor-pointer font-medium">{m.platform_plugins_config_schema()}</summary>
              <pre class="mt-3 overflow-x-auto rounded-lg border bg-surface p-3 font-mono text-xs">{JSON.stringify(
                  editTarget.manifest.config_schema,
                  null,
                  2,
                )}</pre>
            </details>
          {/if}
          {#if configError}<RequestFailure title={m.platform_plugins_config_failed()} message={configError} />{/if}
        </div>
        <Sheet.Footer class="route-overlay-footer pb-[max(0.75rem,env(safe-area-inset-bottom))]">
          <Button
            type="button"
            variant="outline"
            disabled={savingConfig}
            onclick={() => guard.request(() => (editorOpen = false))}>{m.common_cancel()}</Button>
          <Button type="submit" disabled={savingConfig || !baseline.dirty} aria-busy={savingConfig}
            >{#if savingConfig}<Spinner data-icon="inline-start" />{/if}{m.common_save()}</Button>
        </Sheet.Footer>
      </form>
    {/if}
  </Sheet.Content>
</Sheet.Root>
<DraftDiscardDialog {guard} />

<AlertDialog.Root
  open={Boolean(uninstallTarget)}
  onOpenChange={(open) => {
    if (!open && !uninstalling) uninstallTarget = undefined
  }}>
  <AlertDialog.Content
    escapeKeydownBehavior={uninstalling ? 'ignore' : 'close'}
    interactOutsideBehavior={uninstalling ? 'ignore' : 'close'}>
    <AlertDialog.Header>
      <AlertDialog.Title
        >{m.platform_plugins_uninstall_title({ name: uninstallTarget?.manifest.name ?? '' })}</AlertDialog.Title>
      <AlertDialog.Description>{m.platform_plugins_uninstall_description()}</AlertDialog.Description>
    </AlertDialog.Header>
    {#if uninstallError}<RequestFailure message={uninstallError} />{/if}
    <AlertDialog.Footer>
      <AlertDialog.Cancel disabled={uninstalling}>{m.common_cancel()}</AlertDialog.Cancel>
      <Button variant="destructive" disabled={uninstalling} aria-busy={uninstalling} onclick={() => void uninstall()}
        >{#if uninstalling}<Spinner data-icon="inline-start" />{/if}{m.platform_plugins_uninstall()}</Button>
    </AlertDialog.Footer>
  </AlertDialog.Content>
</AlertDialog.Root>

<style>
.platform-plugins-page {
  min-height: 100%;
}
.platform-plugins-workspace {
  display: flex;
  min-height: 0;
  flex: 1;
  flex-direction: column;
}
</style>

<script lang="ts">
import { useQueryClient } from '@tanstack/svelte-query'
import { admin, type CredentialImportRow } from '#lib/admin-client.js'
import * as m from '#lib/paraglide/messages.js'
import { Button } from '#lib/components/ui/button/index.js'
import { Input } from '#lib/components/ui/input/index.js'
import { Label } from '#lib/components/ui/label/index.js'
import { Textarea } from '#lib/components/ui/textarea/index.js'
import { Switch } from '#lib/components/ui/switch/index.js'
import * as Dialog from '#lib/components/ui/dialog/index.js'

let { open = $bindable(false) }: { open?: boolean } = $props()
const client = useQueryClient()
let kind = $state('codex')
let json = $state('')
let useProxy = $state(false)
let rows = $state<(CredentialImportRow & { selected: boolean })[]>([])
let busy = $state(false)
let error = $state('')

function invalidate(): void {
  rows = []
  error = ''
}

function changedOpen(value: boolean): void {
  if (!value && !busy) {
    json = ''
    rows = []
    error = ''
    useProxy = false
  }
}

async function loadFile(event: Event): Promise<void> {
  const element = event.currentTarget as HTMLInputElement
  const file = element.files?.[0]
  invalidate()
  if (!file) return
  if (file.size > 1024 * 1024) {
    error = m.credential_import_size()
    element.value = ''
    return
  }
  busy = true
  try {
    json = await file.text()
  } catch {
    error = m.credential_import_failed()
  } finally {
    element.value = ''
    busy = false
  }
}

async function preview(): Promise<void> {
  if (busy) return
  busy = true
  error = ''
  try {
    if (new TextEncoder().encode(json).byteLength > 1024 * 1024) throw new Error('size')
    const result = await admin.providers.previewImport(kind, json)
    rows = result.rows.map((row) => ({ ...row, selected: row.code === 'IMPORT_READY' }))
  } catch {
    rows = []
    error = m.credential_import_failed()
  } finally {
    busy = false
  }
}

async function commit(): Promise<void> {
  if (busy) return
  busy = true
  error = ''
  try {
    for (const row of rows.filter((row) => row.selected && row.code === 'IMPORT_READY')) {
      try {
        await admin.providers.importCredential(kind, json, row.index, row.label, useProxy)
        row.code = 'IMPORT_DONE'
        row.selected = false
      } catch {
        row.code = 'IMPORT_FAILED'
        row.selected = false
      }
    }
    await client.invalidateQueries({ queryKey: ['providers'] })
    // Remove credential text after submission; retries require a fresh preview.
    json = ''
  } finally {
    busy = false
  }
}

function status(code: string): string {
  if (code === 'IMPORT_READY') return m.credential_import_ready()
  if (code === 'IMPORT_DONE') return m.credential_import_done()
  if (code === 'IMPORT_DUPLICATE') return m.credential_import_duplicate()
  if (code === 'IMPORT_FAILED') return m.credential_import_failed()
  return m.credential_import_unsupported()
}
</script>

<Dialog.Root bind:open onOpenChange={changedOpen}>
  <Dialog.Content
    class="max-h-[85dvh] overflow-y-auto sm:max-w-xl"
    showCloseButton={!busy}
    onInteractOutside={(event: Event) => {
      if (busy) event.preventDefault()
    }}
    onEscapeKeydown={(event: KeyboardEvent) => {
      if (busy) event.preventDefault()
    }}>
    <Dialog.Header>
      <Dialog.Title>{m.credential_import_title()}</Dialog.Title>
      <Dialog.Description>{m.credential_import_description()}</Dialog.Description>
    </Dialog.Header>
    <div class="space-y-2">
      <Label for="credential-import-kind">{m.credential_import_kind()}</Label>
      <select
        id="credential-import-kind"
        class="h-9 w-full rounded-md border bg-background px-3"
        bind:value={kind}
        disabled={busy}
        onchange={invalidate}>
        <option value="codex">Codex · sub2api / Cockpit Tools / CLIProxyAPI / auth.json</option>
        <option value="antigravity">Antigravity / Google · RT JSON</option>
      </select>
    </div>
    <div class="space-y-2">
      <Label for="credential-import-file">{m.credential_import_file()}</Label>
      <Input
        id="credential-import-file"
        type="file"
        accept=".json,.txt,application/json,text/plain"
        disabled={busy}
        onchange={(event: Event) => void loadFile(event)} />
      <Label for="credential-import-json">{m.credential_import_json()}</Label>
      <Textarea
        id="credential-import-json"
        class="min-h-36 font-mono text-xs"
        bind:value={json}
        disabled={busy}
        oninput={invalidate}
        autocomplete="off"
        spellcheck={false} />
      <p class="text-xs text-muted-foreground">{m.credential_import_scope()}</p>
    </div>
    <div class="flex items-center justify-between gap-3">
      <Label for="credential-import-proxy">{m.credential_import_proxy()}</Label>
      <Switch id="credential-import-proxy" bind:checked={useProxy} disabled={busy} />
    </div>
    {#if error}<p role="alert" class="text-sm text-destructive">{error}</p>{/if}
    {#if rows.length}
      <div class="space-y-3 rounded-xl border p-3" aria-label={m.credential_import_preview()}>
        {#each rows as row (row.index)}
          <div class="space-y-1">
            <div class="flex min-w-0 items-center gap-2">
              <input
                type="checkbox"
                bind:checked={row.selected}
                disabled={busy || row.code !== 'IMPORT_READY'}
                aria-label={m.credential_import_select({ index: row.index + 1 })} />
              <Input
                bind:value={row.label}
                maxlength={160}
                disabled={busy || row.code !== 'IMPORT_READY'}
                aria-label={m.credential_import_name({ index: row.index + 1 })} />
            </div>
            <p class="text-xs text-muted-foreground" role="status">
              {status(row.code)}{row.code === 'IMPORT_READY' && !row.has_refresh_token
                ? ` · ${m.credential_import_access_only()}`
                : ''}
            </p>
          </div>
        {/each}
      </div>
    {/if}
    <Dialog.Footer class="flex-wrap gap-2">
      <Button variant="outline" disabled={busy || !json.trim()} onclick={() => void preview()}
        >{m.credential_import_preview()}</Button>
      <Button
        disabled={busy || !rows.some((row) => row.selected && row.code === 'IMPORT_READY')}
        onclick={() => void commit()}>{busy ? m.credential_import_busy() : m.credential_import_confirm()}</Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>

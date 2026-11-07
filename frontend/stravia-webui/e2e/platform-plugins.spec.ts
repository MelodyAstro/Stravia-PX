import { expect, test } from '@playwright/test'

import type { PlatformPluginSummary } from '../src/lib/types'
import { prepareApp } from './prepare-app'

const initialPlugin: PlatformPluginSummary = {
  manifest: {
    id: 'example.policy-tools',
    name: 'Policy and tools',
    version: '1.0.0',
    api_version: 1,
    hooks: ['request', 'client_output'],
    tools: [{ name: 'echo', description: 'Returns the input.', parameters: { type: 'object' }, parallel_safe: true }],
  },
  sha256: 'a'.repeat(64),
  enabled: false,
  config: { blocked_models: [] },
}

test('plugin settings preserve failed drafts and enabling only changes saved enabled state', async ({ page }) => {
  await prepareApp(page)
  let plugin = structuredClone(initialPlugin)
  let failConfig = true
  let failEnabled = true
  const writes: unknown[] = []
  await page.route('**/api/v1/platform-plugins', (route) => route.fulfill({ json: { data: [plugin] } }))
  await page.route('**/api/v1/platform-plugins/example.policy-tools/settings', async (route) => {
    const input = route.request().postDataJSON()
    writes.push(input)
    if ('enabled' in input && failEnabled) {
      failEnabled = false
      await route.fulfill({ status: 400, json: { error: 'Enable rejected' } })
      return
    }
    if ('config' in input && failConfig) {
      failConfig = false
      await route.fulfill({ status: 400, json: { error: 'Configuration rejected' } })
      return
    }
    plugin = { ...plugin, ...input }
    await route.fulfill({ json: { data: plugin } })
  })

  await page.goto('/platform-plugins')
  const row = page.locator('[data-plugin-id="example.policy-tools"]')
  const enabled = row.getByRole('switch')
  await expect(enabled).not.toBeChecked()
  await row.getByRole('button', { name: 'Configure', exact: true }).click()
  const editor = page.getByRole('dialog', { name: initialPlugin.manifest.name })
  const config = editor.getByLabel('Configuration (JSON)', { exact: true })
  await config.fill('{broken')
  await editor.getByRole('button', { name: 'Save', exact: true }).click()
  await expect(editor.getByText('Enter valid JSON before saving.')).toBeVisible()
  expect(writes).toHaveLength(0)
  await config.fill('{"blocked_models":["legacy"]}')
  await editor.getByRole('button', { name: 'Save', exact: true }).click()
  await expect(editor.getByText('Configuration rejected')).toBeVisible()
  await expect(config).toHaveValue('{"blocked_models":["legacy"]}')
  await editor.getByRole('button', { name: 'Save', exact: true }).click()
  await expect(editor).toHaveCount(0)
  await enabled.click()
  await expect(row.getByText('Enable rejected')).toBeVisible()
  await expect(enabled).not.toBeChecked()
  await enabled.click()
  await expect(enabled).toBeChecked()
  expect(writes).toEqual([
    { config: { blocked_models: ['legacy'] } },
    { config: { blocked_models: ['legacy'] } },
    { enabled: true },
    { enabled: true },
  ])
})

test('installing a plugin reviews the disabled result and uninstall requires confirmation', async ({ page }) => {
  await page.setViewportSize({ width: 320, height: 700 })
  await prepareApp(page)
  let installed = false
  let deletions = 0
  await page.route('**/api/v1/platform-plugins', async (route) => {
    if (route.request().method() === 'POST') {
      expect(route.request().headers()['content-type']).toBe('application/wasm')
      expect(route.request().headers()['x-stravia-csrf']).toBe('1')
      installed = true
      await route.fulfill({ json: { data: initialPlugin } })
    } else {
      await route.fulfill({ json: { data: installed ? [initialPlugin] : [] } })
    }
  })
  await page.route('**/api/v1/platform-plugins/example.policy-tools', async (route) => {
    expect(route.request().method()).toBe('DELETE')
    deletions += 1
    installed = false
    await route.fulfill({ json: { data: null } })
  })

  await page.goto('/platform-plugins')
  await expect(page.getByText('Install your first platform plugin')).toBeVisible()
  await page
    .locator('input[type="file"]')
    .setInputFiles({
      name: 'policy-tools.wasm',
      mimeType: 'application/wasm',
      buffer: Buffer.from([0, 97, 115, 109, 13, 0, 1, 0]),
    })
  const editor = page.getByRole('dialog', { name: initialPlugin.manifest.name })
  await expect(editor).toBeVisible()
  await expect(editor.getByText('echo', { exact: true })).toBeVisible()
  await expect(editor.getByText(/This plugin is disabled/)).toBeVisible()
  await editor.getByRole('button', { name: 'Cancel', exact: true }).click()
  const row = page.locator('[data-plugin-id="example.policy-tools"]')
  await expect(row.getByRole('switch')).not.toBeChecked()
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true)
  await row.getByRole('button', { name: 'Uninstall', exact: true }).click()
  const confirmation = page.getByRole('alertdialog', { name: 'Uninstall Policy and tools?' })
  await confirmation.getByRole('button', { name: 'Cancel', exact: true }).click()
  expect(deletions).toBe(0)
  await row.getByRole('button', { name: 'Uninstall', exact: true }).click()
  await confirmation.getByRole('button', { name: 'Uninstall', exact: true }).click()
  await expect(page.getByText('Install your first platform plugin')).toBeVisible()
  expect(deletions).toBe(1)
})

test('Chinese plugin configuration remains usable in a narrow dark layout', async ({ page }) => {
  await page.setViewportSize({ width: 320, height: 700 })
  await page.emulateMedia({ colorScheme: 'dark' })
  await prepareApp(page, 'zh-CN')
  const plugin = {
    ...initialPlugin,
    manifest: {
      ...initialPlugin.manifest,
      name: '请求策略与工具',
      config_schema: { type: 'object', properties: { blocked_models: { type: 'array', items: { type: 'string' } } } },
    },
  }
  await page.route('**/api/v1/platform-plugins', (route) => route.fulfill({ json: { data: [plugin] } }))
  await page.goto('/platform-plugins')
  await expect(page.getByRole('heading', { name: '平台插件', exact: true })).toBeVisible()
  await page.getByRole('button', { name: '配置', exact: true }).click()
  const editor = page.getByRole('dialog', { name: plugin.manifest.name })
  await expect(editor.getByLabel('配置（JSON）', { exact: true })).toBeVisible()
  await editor.getByText('配置格式', { exact: true }).click()
  await expect(editor.locator('pre')).toContainText('blocked_models')
  expect((await editor.boundingBox())?.width).toBe(320)
  await expect(editor.getByRole('button', { name: '保存', exact: true })).toBeVisible()
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true)
  await editor.locator('.route-overlay-body').evaluate((element) => {
    element.scrollTop = 0
  })
  await page.screenshot({ path: '../../.scratch/platform-plugins-zh-dark-mobile.png' })
})

test('updating a row sends its expected identity and preserves an enabled plugin when rejected', async ({ page }) => {
  await prepareApp(page)
  const plugin = { ...initialPlugin, enabled: true }
  let generalInstalls = 0
  let targetedUpdates = 0
  await page.route('**/api/v1/platform-plugins', async (route) => {
    if (route.request().method() === 'POST') generalInstalls += 1
    await route.fulfill({ json: { data: [plugin] } })
  })
  await page.route('**/api/v1/platform-plugins/example.policy-tools', async (route) => {
    expect(route.request().method()).toBe('POST')
    expect(route.request().headers()['content-type']).toBe('application/wasm')
    targetedUpdates += 1
    await route.fulfill({ status: 400, json: { error: 'Plugin identity does not match the update target' } })
  })

  await page.goto('/platform-plugins')
  const row = page.locator('[data-plugin-id="example.policy-tools"]')
  await expect(row.getByRole('switch')).toBeChecked()
  const fileChooser = page.waitForEvent('filechooser')
  await row.getByRole('button', { name: 'Update from file', exact: true }).click()
  await (
    await fileChooser
  ).setFiles({
    name: 'different-plugin.wasm',
    mimeType: 'application/wasm',
    buffer: Buffer.from([0, 97, 115, 109, 13, 0, 1, 0]),
  })
  await expect(page.getByText('Plugin identity does not match the update target', { exact: true })).toBeVisible()
  expect(targetedUpdates).toBe(1)
  expect(generalInstalls).toBe(0)
  await expect(row.getByRole('switch')).toBeChecked()
  await expect(page.getByRole('dialog')).toHaveCount(0)
  await row.getByRole('button', { name: 'Configure', exact: true }).click()
  const editor = page.getByRole('dialog', { name: plugin.manifest.name })
  await expect(editor.getByLabel('Configuration (JSON)', { exact: true })).toHaveValue(
    JSON.stringify(plugin.config, null, 2),
  )
})

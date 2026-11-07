import { expect, test } from '@playwright/test'
import { prepareApp } from './prepare-app'
import type { RequestModelsView } from '../src/lib/types/api-key'
import en from '../messages/en-US.json'
import zh from '../messages/zh-CN.json'

for (const width of [320, 500, 1280]) {
  test(`request mappings preserve custom names and recognize requests at ${width}px`, async ({ page }) => {
    const locale = width === 500 ? 'zh-CN' : 'en-US'
    const t = locale === 'zh-CN' ? zh : en
    await prepareApp(page, locale)
    await page.emulateMedia({ colorScheme: width === 500 ? 'dark' : 'light' })
    await page.setViewportSize({ width, height: 900 })
    const writes: unknown[] = []
    const key = {
      id: 'mapping-key',
      key: 'sk-isolated-ui-fixture',
      name: 'Private fixture',
      rpm_limit: null,
      is_enabled: true,
      mcp_access_enabled: false,
      transparent_injection_enabled: false,
      inject_web_search: false,
      inject_media_generation: false,
      inject_media_understanding: false,
      model_ids: ['private-id'],
    }
    const view: RequestModelsView = {
      config: { enabled: false, mappings: [] },
      eligible_models: [{ id: 'private-id', model_id: 'private/gpt-6-luna', display_name: 'Private Luna' }],
      capture: { active: false, expires_at: null, samples: [], dropped_samples: 0 },
      suggestions: [],
    }
    await page.route('**/api/v1/api-keys', (route) => route.fulfill({ json: { data: [key] } }))
    await page.route('**/api/v1/models', (route) => route.fulfill({ json: { data: [] } }))
    await page.route('**/api/v1/api-keys/mapping-key/request-models', async (route) => {
      if (route.request().method() === 'PUT') {
        view.config = route.request().postDataJSON()
        writes.push(view.config)
      }
      await route.fulfill({ json: { data: view } })
    })
    await page.route('**/api/v1/api-keys/mapping-key/request-models/capture', async (route) => {
      const enabled = route.request().method() === 'POST' && route.request().postDataJSON().enabled
      view.capture.active = enabled
      view.capture.samples = enabled
        ? [
            {
              request_model: 'codex-client-model',
              protocol: 'open-responses/2026-04-24',
              transport: 'websocket',
              stream: true,
              input_items: 11,
              tools: 0,
              count: 1,
            },
          ]
        : []
      view.suggestions = enabled
        ? [{ request_model: 'codex-client-model', target_model_id: 'private-id', candidates: ['private-id'] }]
        : []
      await route.fulfill({ json: { data: view } })
    })
    await page.goto('/api-keys/mapping-key')
    const editor = page.locator('#request-models')
    await expect(editor.getByRole('heading', { name: t.request_models_title, exact: true })).toBeVisible()
    await editor.getByRole('button', { name: t.request_models_add, exact: true }).click()
    await editor.getByLabel(t.request_models_client_name).fill('my-custom-luna')
    await editor.getByRole('button', { name: t.request_models_target, exact: true }).click()
    await page.getByRole('option', { name: 'private/gpt-6-luna', exact: true }).click()
    await editor.getByRole('switch').click()
    await editor.getByRole('button', { name: t.request_models_save, exact: true }).click()
    await expect.poll(() => writes.length).toBe(1)
    expect(writes[0]).toEqual({
      enabled: true,
      mappings: [{ request_model: 'my-custom-luna', target_model_id: 'private-id' }],
    })
    await editor.getByRole('button', { name: t.request_models_start }).click()
    await expect(editor.getByText('codex-client-model', { exact: true })).toBeVisible()
    await editor.getByRole('button', { name: t.request_models_use_sample }).click()
    await expect(editor.getByLabel(t.request_models_client_name).nth(1)).toHaveValue('codex-client-model')
    await expect(editor.getByRole('button', { name: t.request_models_target, exact: true }).nth(1)).toHaveText(
      'private/gpt-6-luna',
    )
    expect(writes.length).toBe(1)
    await editor.getByRole('button', { name: t.request_models_save, exact: true }).click()
    await expect.poll(() => writes.length).toBe(2)
    await editor.getByRole('button', { name: t.request_models_clear }).click()
    await expect(editor.getByText('codex-client-model', { exact: true })).toHaveCount(0)
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true)
    await page.screenshot({ path: `test-results/request-models-${width}-${locale}.png` })
  })
}

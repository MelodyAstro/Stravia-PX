import { expect, test } from '@playwright/test'
import { prepareApp } from './prepare-app'
import { readFile } from 'node:fs/promises'

test('previews OAuth imports, skips duplicates and requires explicit confirmation', async ({ page }) => {
  await prepareApp(page)
  const commits: Record<string, unknown>[] = []
  await page.route('**/api/v1/providers/credential-import/preview', (route) =>
    route.fulfill({
      json: {
        data: {
          rows: [
            { index: 0, label: 'New account', code: 'IMPORT_READY', has_refresh_token: true },
            { index: 1, label: 'Existing account', code: 'IMPORT_DUPLICATE', has_refresh_token: true },
            { index: 2, label: '#3', code: 'IMPORT_UNSUPPORTED_AUTH_MODE', has_refresh_token: false },
          ],
        },
      },
    }),
  )
  await page.route('**/api/v1/providers/credential-import', (route) => {
    commits.push(route.request().postDataJSON())
    return route.fulfill({ json: { data: { id: 'imported-fixture' } } })
  })
  await page.goto('/providers')
  await page.getByRole('button', { name: 'Import credentials', exact: true }).click()
  const dialog = page.getByRole('dialog')
  await dialog.getByLabel('Or paste JSON / Google refresh token').fill('[{"refresh_token":"fixture-private-token"}]')
  await expect(dialog.getByRole('button', { name: 'Confirm selected imports' })).toBeDisabled()
  await dialog.getByRole('button', { name: 'Preview', exact: true }).click()
  await expect(dialog.getByText('Existing account — skipped')).toBeVisible()
  expect(commits).toHaveLength(0)
  await expect(dialog.getByRole('checkbox', { name: 'Select account 2' })).toBeDisabled()
  await dialog.getByRole('textbox', { name: 'Account 1 name' }).fill('Personal Codex')
  await dialog.getByRole('button', { name: 'Confirm selected imports' }).click()
  await expect(dialog.getByText('Imported', { exact: true })).toBeVisible()
  expect(commits).toHaveLength(1)
  expect(commits[0]).toMatchObject({
    kind: 'codex',
    index: 0,
    name: 'Personal Codex',
    confirmed: true,
    use_proxy: false,
  })
  await expect(dialog.getByLabel('Or paste JSON / Google refresh token')).toHaveValue('')
  await page.setViewportSize({ width: 375, height: 760 })
  expect(await dialog.evaluate((node) => node.scrollWidth - node.clientWidth)).toBeLessThanOrEqual(1)
  await dialog.getByRole('button', { name: 'Close', exact: true }).click()
  await page.getByRole('button', { name: 'Import credentials', exact: true }).click()
  await expect(page.getByRole('dialog').getByLabel('Or paste JSON / Google refresh token')).toHaveValue('')
})

test('Google RT import accepts a JSON file and invalidates preview on edits', async ({ page }) => {
  await prepareApp(page)
  let previewKind = ''
  await page.route('**/api/v1/providers/credential-import/preview', (route) => {
    previewKind = route.request().postDataJSON().kind
    return route.fulfill({
      json: { data: { rows: [{ index: 0, label: 'Google account', code: 'IMPORT_READY', has_refresh_token: true }] } },
    })
  })
  await page.goto('/providers')
  await page.getByRole('button', { name: 'Import credentials', exact: true }).click()
  const dialog = page.getByRole('dialog')
  await dialog.getByLabel('Account type', { exact: true }).selectOption('antigravity')
  await dialog
    .getByLabel('Choose a JSON file (up to 1 MiB)')
    .setInputFiles({
      name: 'rt.json',
      mimeType: 'application/json',
      buffer: Buffer.from('{"mail":"fixture@example.invalid","refresh_token":"fixture-google-rt"}'),
    })
  await dialog.getByRole('button', { name: 'Preview', exact: true }).click()
  await expect(dialog.getByText('Ready to import', { exact: true })).toBeVisible()
  expect(previewKind).toBe('antigravity')
  await dialog.getByLabel('Or paste JSON / Google refresh token').fill('changed-fixture-rt')
  await expect(dialog.getByRole('button', { name: 'Confirm selected imports' })).toBeDisabled()
})

test('RT backup downloads credentials only after confirmation', async ({ page }) => {
  await prepareApp(page)
  let exports = 0
  await page.route('**/api/v1/providers', (route) =>
    route.fulfill({
      json: {
        data: [
          {
            id: 'google-fixture',
            name: 'Google fixture',
            vendor: 'antigravity',
            channel: 'oauth',
            protocol: 'google-gemini-generate-content-v1beta',
            base_url: 'https://fixture.invalid',
            is_enabled: true,
            use_proxy: false,
            credential_status: 'valid',
            oauth_status: 'connected',
            configured_credential_fields: [],
            allowance_suspension: null,
            cost_plan: null,
            created_at: '2026-11-11T00:00:00Z',
            updated_at: '2026-11-11T00:00:00Z',
          },
        ],
      },
    }),
  )
  await page.route('**/api/v1/providers/google-fixture/rt-backup', (route) => {
    expect(route.request().postDataJSON()).toEqual({ confirmed: true })
    exports++
    return route.fulfill({ json: { data: { type: 'antigravity', refresh_token: 'fixture-backup-rt' } } })
  })
  await page.goto('/providers')
  const openBackup = async () => {
    await page.getByRole('button', { name: 'More actions for Google fixture' }).first().click()
    await page.getByRole('menuitem', { name: 'Back up RT JSON' }).click()
  }
  await openBackup()
  const confirmation = page.getByRole('alertdialog')
  await expect(confirmation).toContainText('contains a refresh token')
  expect(exports).toBe(0)
  await confirmation.getByRole('button', { name: 'Cancel', exact: true }).click()
  expect(exports).toBe(0)
  await openBackup()
  const downloaded = page.waitForEvent('download')
  await confirmation.getByRole('button', { name: 'Download RT JSON' }).click()
  const file = await downloaded
  expect(file.suggestedFilename()).toBe('antigravity-rt.json')
  expect(JSON.parse(await readFile(await file.path(), 'utf8'))).toEqual({
    type: 'antigravity',
    refresh_token: 'fixture-backup-rt',
  })
  expect(exports).toBe(1)
})

import { expect, test } from '@playwright/test'
import { prepareApp } from './prepare-app'

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

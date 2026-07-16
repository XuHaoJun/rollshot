import { test, expect } from '@playwright/test';
import { pathToFileURL } from 'node:url';
import { resolve } from 'node:path';
import { rename } from 'node:fs/promises';

const guideDir = resolve('.tmp/guide');
const guideUrl = pathToFileURL(resolve(guideDir, 'index.html')).href;

test.beforeEach(async ({ page }) => {
  const nonFileRequests = [];
  page.on('request', request => {
    const protocol = new URL(request.url()).protocol;
    if (protocol !== 'file:' && protocol !== 'data:') nonFileRequests.push(request.url());
  });
  await page.goto(guideUrl);
  await expect(page.getByTestId('step-progress')).toHaveText('Step 1 of 4');
  expect(nonFileRequests).toEqual([]);
});

test('navigation, keyboard, and zoom stay synchronized', async ({ page }) => {
  await page.getByRole('button', { name: 'Next step' }).click();
  await expect(page.getByTestId('step-progress')).toHaveText('Step 2 of 4');
  await page.keyboard.press('ArrowRight');
  await expect(page.getByTestId('step-progress')).toHaveText('Step 3 of 4');
  await page.keyboard.press('+');
  await expect(page.getByTestId('zoom-value')).toHaveText('125%');
  await page.keyboard.press('0');
  await expect(page.getByTestId('zoom-value')).toHaveText('100%');
});

test('search opens annotation matches and does not execute guide markup', async ({ page }) => {
  await page.getByRole('searchbox', { name: 'Search guide' }).fill('settings');
  await page.locator('#step-list button').filter({ hasText: /settings/i }).first().click();
  await expect(page.getByRole('dialog')).toContainText('Open Settings');
  expect(await page.evaluate(() => globalThis.pwned)).toBeUndefined();
});

test('clipboard rejection exposes honest manual copy', async ({ page }) => {
  await page.addInitScript(() => Object.defineProperty(navigator, 'clipboard', {
    configurable: true,
    value: { writeText: () => Promise.reject(new DOMException('denied', 'NotAllowedError')) }
  }));
  await page.reload();
  await page.getByRole('button', { name: 'Copy step text' }).click();
  const fallback = page.getByRole('textbox', { name: 'Step text for manual copy' });
  await expect(fallback).toBeFocused();
  await expect(page.getByRole('status')).toContainText('Press Ctrl/Cmd+C');
});

test('clipboard success copies exact step text and reports success', async ({ page }) => {
  let copied = null;
  await page.addInitScript(() => Object.defineProperty(navigator, 'clipboard', {
    configurable: true,
    value: { writeText: text => { globalThis.__copied = text; return Promise.resolve(); } }
  }));
  await page.reload();
  await page.getByRole('button', { name: 'Copy step text' }).click();
  copied = await page.evaluate(() => globalThis.__copied);
  expect(copied).toContain('Step 1: Open Settings');
  await expect(page.getByRole('status')).toHaveText('Copied');
  await expect(page.getByRole('textbox', { name: 'Step text for manual copy' })).toBeHidden();
});

test('missing image is local to one step', async ({ page }) => {
  const image = resolve(guideDir, 'keyframes/003.png');
  const hidden = `${image}.missing`;
  await rename(image, hidden);
  try {
    await page.goto(guideUrl);
    await page.getByRole('button', { name: /Step 3/ }).click();
    await expect(page.getByText('Image unavailable')).toBeVisible();
    await page.getByRole('button', { name: /Step 2/ }).click();
    await expect(page.getByRole('img')).toBeVisible();
  } finally {
    await rename(hidden, image);
  }
});

test('guide-title search appears once and slash focuses search', async ({ page }) => {
  await page.keyboard.press('/');
  await expect(page.getByRole('searchbox', { name: 'Search guide' })).toBeFocused();
  await page.keyboard.type('Checkout failure');
  await expect(page.locator('[data-result-kind="guide"]')).toHaveCount(1);
  await expect(page.locator('#guide-title mark')).toHaveText('Checkout failure');
});

test('popover replaces, closes on Escape, and shortcuts ignore text entry', async ({ page }) => {
  const hotspots = page.locator('.hotspot');
  await hotspots.nth(0).click();
  await expect(hotspots.nth(0)).toBeFocused();
  const firstText = await page.getByRole('dialog').textContent();
  await hotspots.nth(1).click();
  await expect(hotspots.nth(1)).toBeFocused();
  await expect(page.getByRole('dialog')).not.toHaveText(firstText);
  await page.keyboard.press('Escape');
  await expect(page.getByRole('dialog')).toBeHidden();
  await expect(hotspots.nth(1)).toBeFocused();
  await page.getByRole('searchbox', { name: 'Search guide' }).fill('x');
  await page.keyboard.press('ArrowRight');
  await expect(page.getByTestId('step-progress')).toHaveText('Step 1 of 4');
});

test('narrow layout uses drawer and below-image explanations', async ({ page }) => {
  await page.setViewportSize({ width: 600, height: 800 });
  await expect(page.getByRole('button', { name: 'Toggle steps' })).toBeVisible();
  await page.locator('.hotspot').first().click();
  await expect(page.getByRole('dialog')).toHaveCSS('position', 'static');
});

test('theme, reduced motion, skip link, and focus visibility are honored', async ({ page }) => {
  await page.emulateMedia({ colorScheme: 'dark', reducedMotion: 'reduce' });
  await page.reload();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
  await expect(page.locator('html')).toHaveAttribute('data-motion', 'reduce');
  await page.keyboard.press('Tab');
  await expect(page.getByRole('link', { name: 'Skip to current step' })).toBeFocused();
  await page.emulateMedia({ colorScheme: 'light', reducedMotion: 'no-preference' });
  await page.reload();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'light');
  await expect(page.locator('html')).toHaveAttribute('data-motion', 'full');
});

test('hotspot percentages stay aligned while shell zooms', async ({ page }) => {
  await expect(page.locator('#step-image')).toBeVisible();
  const hotspot = page.locator('.hotspot').first();
  await hotspot.click();
  await expect(page.getByRole('dialog')).toBeVisible();
  const popoverBefore = await page.getByRole('dialog').boundingBox();
  const before = await hotspot.boundingBox();
  const shell = page.locator('#image-shell');
  await page.getByRole('button', { name: 'Zoom in' }).click();
  await page.getByRole('button', { name: 'Zoom in' }).click();
  await expect(page.getByTestId('zoom-value')).toHaveText('150%');
  const after = await hotspot.boundingBox();
  const shellAfter = await shell.boundingBox();
  expect(after.width).toBeGreaterThan(before.width);
  expect(after.x).toBeGreaterThanOrEqual(shellAfter.x - 1);
  expect(after.x + after.width).toBeLessThanOrEqual(shellAfter.x + shellAfter.width + 2);
  const popoverAfter = await page.getByRole('dialog').boundingBox();
  expect(popoverAfter.x).not.toBe(popoverBefore.x);
});

test('initial load requests only current and adjacent keyframes', async ({ page }) => {
  const images = [];
  page.on('request', request => {
    if (request.url().endsWith('.png')) images.push(request.url());
  });
  await page.goto(guideUrl);
  await page.waitForLoadState('load');
  expect(images.some(url => url.endsWith('/004.png'))).toBe(false);
});

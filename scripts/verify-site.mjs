// Checks the GitHub Pages site in site/ the way a visitor would see it.
//
//   npm run site:verify
//
// Serves site/ under /node-modules-cleaner/ (the Pages project path) and, for both
// languages, checks: no console errors, every image decodes, every internal link and
// anchor resolves, the language switch points at the other language, the two versions
// have the same structure, nothing overflows at 390px, and the lightbox opens and closes.
// Preview captures land in node_modules/.cache/site-preview/ for a visual once-over.

import { createReadStream, existsSync, mkdirSync, statSync } from 'node:fs';
import { createServer } from 'node:http';
import { dirname, extname, join, normalize, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const siteDir = join(root, 'site');
const previewDir = join(root, 'node_modules', '.cache', 'site-preview');
const BASE = '/node-modules-cleaner/';

const TYPES = {
  '.html': 'text/html; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.png': 'image/png',
  '.webp': 'image/webp',
};

function serve() {
  const server = createServer((request, response) => {
    const url = new URL(request.url, 'http://localhost');
    if (!url.pathname.startsWith(BASE)) {
      response.writeHead(404).end();
      return;
    }
    let file = normalize(join(siteDir, decodeURIComponent(url.pathname.slice(BASE.length))));
    if (!file.startsWith(siteDir)) {
      response.writeHead(403).end();
      return;
    }
    if (existsSync(file) && statSync(file).isDirectory()) file = join(file, 'index.html');
    if (!existsSync(file)) {
      response.writeHead(404).end();
      return;
    }
    response.writeHead(200, { 'content-type': TYPES[extname(file)] ?? 'application/octet-stream' });
    createReadStream(file).pipe(response);
  });
  return new Promise((done) => server.listen(0, () => done(server)));
}

const failures = [];
const fail = (page, message) => failures.push(`[${page}] ${message}`);

async function outline(page) {
  return page.evaluate(() => ({
    lang: document.documentElement.lang,
    sections: [...document.querySelectorAll('main section')].map((s) => s.id || '(hero)'),
    headings: document.querySelectorAll('h1, h2, h3').length,
    images: document.querySelectorAll('img').length,
    listItems: document.querySelectorAll('li').length,
    externalLinks: [...document.querySelectorAll('a[href^="http"]')].map((a) => a.href),
    hreflang: [...document.querySelectorAll('link[rel="alternate"]')].map((l) => `${l.hreflang}=${l.href}`),
  }));
}

async function checkPage(context, origin, path, name, counterpart) {
  const page = await context.newPage();
  const errors = [];
  page.on('pageerror', (error) => errors.push(error.message));
  page.on('console', (message) => {
    if (message.type() === 'error') errors.push(message.text());
  });
  const missing = [];
  page.on('response', (response) => {
    if (response.status() >= 400) missing.push(`${response.status()} ${response.url()}`);
  });

  await page.goto(`${origin}${path}`, { waitUntil: 'networkidle' });

  // Pull every lazy image in, then confirm each one actually decoded.
  await page.evaluate(async () => {
    for (const image of document.images) image.loading = 'eager';
    await Promise.all([...document.images].filter((i) => i.getAttribute('src')).map((i) => i.decode().catch(() => {})));
  });
  const broken = await page.evaluate(() => [...document.images]
    .filter((image) => image.getAttribute('src') && image.naturalWidth === 0)
    .map((image) => image.getAttribute('src')));
  broken.forEach((src) => fail(name, `image did not load: ${src}`));

  const missingAlt = await page.evaluate(() => [...document.images]
    .filter((image) => image.getAttribute('src') && !image.hasAttribute('alt'))
    .map((image) => image.getAttribute('src')));
  missingAlt.forEach((src) => fail(name, `image without alt: ${src}`));

  // Internal links: files must exist, fragments must point at an element.
  const links = await page.evaluate(() => [...document.querySelectorAll('a[href]')]
    .map((a) => ({ raw: a.getAttribute('href'), href: a.href }))
    .filter((link) => !/^https?:\/\/(?!localhost|127\.0\.0\.1)/.test(link.href)));
  for (const link of links) {
    const url = new URL(link.href);
    if (url.hash && url.pathname === new URL(page.url()).pathname) {
      const found = await page.locator(url.hash).count();
      if (found === 0) fail(name, `anchor has no target: ${link.raw}`);
      continue;
    }
    const response = await page.request.get(link.href);
    if (!response.ok()) fail(name, `link ${link.raw} → ${response.status()}`);
  }

  const switchHref = await page.locator('.lang-switch').evaluate((a) => a.href);
  if (new URL(switchHref).pathname !== counterpart) {
    fail(name, `language switch points at ${new URL(switchHref).pathname}, expected ${counterpart}`);
  }

  // Lightbox: opens on click, closes on Escape, and hands focus back.
  const firstThumb = page.locator('[data-lightbox]').first();
  await firstThumb.scrollIntoViewIfNeeded();
  await firstThumb.click();
  if (!(await page.locator('dialog.lightbox[open]').isVisible())) fail(name, 'lightbox did not open');
  await page.keyboard.press('Escape');
  if (await page.locator('dialog.lightbox[open]').count()) fail(name, 'lightbox did not close on Escape');

  const result = await outline(page);
  await page.evaluate(() => window.scrollTo(0, 0));
  await page.screenshot({ path: join(previewDir, `${name}-desktop.png`), fullPage: true });

  missing.forEach((line) => fail(name, `HTTP ${line}`));
  errors.forEach((line) => fail(name, `console: ${line}`));
  await page.close();
  return result;
}

async function checkMobile(browser, origin, path, name) {
  const context = await browser.newContext({ viewport: { width: 390, height: 844 }, deviceScaleFactor: 2, isMobile: true, hasTouch: true });
  const page = await context.newPage();
  await page.goto(`${origin}${path}`, { waitUntil: 'networkidle' });
  const overflow = await page.evaluate(() => {
    const width = document.documentElement.clientWidth;
    return [...document.querySelectorAll('body *')]
      .filter((element) => {
        if (element.closest('pre, .table-wrap')) return false; // these scroll on purpose
        return element.getBoundingClientRect().right > width + 1;
      })
      .slice(0, 5)
      .map((element) => `${element.tagName.toLowerCase()}.${element.className}`);
  });
  overflow.forEach((element) => fail(`${name}-mobile`, `wider than the viewport: ${element}`));
  await page.screenshot({ path: join(previewDir, `${name}-mobile.png`) });
  await page.screenshot({ path: join(previewDir, `${name}-mobile-full.png`), fullPage: true });
  await context.close();
}

async function checkDark(browser, origin, path, name) {
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, colorScheme: 'dark' });
  const page = await context.newPage();
  await page.goto(`${origin}${path}`, { waitUntil: 'networkidle' });
  const background = await page.evaluate(() => getComputedStyle(document.body).backgroundColor);
  if (background === 'rgb(249, 250, 251)') fail(`${name}-dark`, 'dark mode did not apply');
  await page.screenshot({ path: join(previewDir, `${name}-dark.png`) });
  await context.close();
}

async function main() {
  mkdirSync(previewDir, { recursive: true });
  const server = await serve();
  const origin = `http://localhost:${server.address().port}`;
  const browser = await chromium.launch();

  try {
    const context = await browser.newContext({ viewport: { width: 1440, height: 900 } });
    const pl = await checkPage(context, origin, BASE, 'pl', `${BASE}en/`);
    const en = await checkPage(context, origin, `${BASE}en/`, 'en', BASE);
    await context.close();

    if (pl.lang !== 'pl') fail('pl', `<html lang="${pl.lang}">`);
    if (en.lang !== 'en') fail('en', `<html lang="${en.lang}">`);
    for (const key of ['sections', 'headings', 'images', 'listItems', 'externalLinks', 'hreflang']) {
      const a = JSON.stringify(pl[key]);
      const b = JSON.stringify(en[key]);
      // Section ids are translated slugs, so only their count has to match.
      const same = key === 'sections' ? pl[key].length === en[key].length : a === b;
      if (!same) fail('pl↔en', `${key} differ:\n  pl ${a}\n  en ${b}`);
    }

    for (const [path, name] of [[BASE, 'pl'], [`${BASE}en/`, 'en']]) {
      await checkMobile(browser, origin, path, name);
      await checkDark(browser, origin, path, name);
    }
  } finally {
    await browser.close();
    server.close();
  }

  if (failures.length > 0) {
    console.error(`${failures.length} problem(s):\n${failures.map((line) => `  - ${line}`).join('\n')}`);
    process.exit(1);
  }
  console.log(`Site OK — previews in ${previewDir.replace(`${root}/`, '')}`);
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});

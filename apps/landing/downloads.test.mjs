import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { runInNewContext } from 'node:vm';
const source = readFileSync(new URL('./public/downloads.js', import.meta.url), 'utf8');
const html = readFileSync(new URL('./public/index.html', import.meta.url), 'utf8');
async function load(navigator, version = '0.2.109', fail = false) {
  const links = Object.fromEntries(['nav-download', 'hero-download', 'closing-download'].map(id => [id, { href: '#downloads', setAttribute() {} }]));
  const choices = ['macos', 'windows', 'windows-portable', 'linux', 'linux-arm'].map(platformDownload => ({ dataset: { platformDownload } }));
  const files = ['macos-arm64.dmg', 'windows-x86_64-setup.exe', 'windows-x86_64.zip', 'linux-x86_64.tar.gz', 'linux-aarch64.tar.gz'];
  const release = { tag_name: `v${version}`, assets: files.map(file => ({ name: `zeren-${version}-${file}` })) };
  runInNewContext(source, { navigator, document: { getElementById: id => links[id], querySelectorAll: () => choices }, fetch: () => fail ? Promise.reject() : Promise.resolve({ ok: true, json: () => Promise.resolve(release) }) });
  await new Promise(resolve => setImmediate(resolve));
  return { links, choices };
}
for (const [platform, userAgent, file] of [
  ['MacIntel', 'Macintosh', 'macos-arm64.dmg'], ['Win32', 'Windows NT', 'windows-x86_64-setup.exe'],
  ['Linux x86_64', 'Linux', 'linux-x86_64.tar.gz'], ['Linux aarch64', 'Linux', 'linux-aarch64.tar.gz'],
]) test(`desktop download: ${platform}`, async () => {
  const { links, choices } = await load({ platform, userAgent });
  for (const id of ['nav-download', 'hero-download', 'closing-download']) assert.equal(links[id].href, `https://github.com/femboypuppy/zeren/releases/download/v0.2.109/zeren-0.2.109-${file}`);
  assert.equal(choices.length, 5);
  for (const choice of choices) assert.match(choice.href, /^https:\/\/github.com\/femboypuppy\/zeren\/releases\/download\/v0.2.109\/zeren-0.2.109-/);
});
for (const navigator of [{ platform: 'Linux', userAgent: 'Android' }, { platform: 'MacIntel', maxTouchPoints: 5 }, { platform: 'iPhone' }, {}]) test(`mobile/unknown stays at chooser: ${JSON.stringify(navigator)}`, async () => {
  const { links } = await load(navigator);
  assert.equal(links['hero-download'].href, '#downloads');
});
for (const [version, fail] of [['<script>alert(1)</script>', false], ['', true]]) test(`safe fallback: ${version}`, async () => {
  const { links } = await load({ platform: 'Win32' }, version, fail);
  assert.equal(links['hero-download'].href, '#downloads');
});
test('HTML retains all five explicit downloads without JavaScript', () => {
  assert.equal((html.match(/data-platform-download=/g) || []).length, 5);
  assert.ok(html.includes('https://github.com/femboypuppy/zeren/releases/latest'));
  assert.ok(html.includes('id="downloads"'));
  assert.ok(html.includes('href="#downloads">All downloads'));
});

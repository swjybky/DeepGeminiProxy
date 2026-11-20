// Run: node --test src/utils/__tests__/miniWindow.test.mjs (Node.js 22.18+)
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { runInNewContext } from 'node:vm';
import { isMiniWindow } from '../env.ts';

const html = readFileSync(new URL('../../../index.html', import.meta.url), 'utf8');
const bootScript = html.match(/<script>([\s\S]*?)<\/script>/)?.[1];
assert.ok(bootScript, 'Expected the pre-paint bootstrap in index.html');

const cases = [
    ['/accounts?pool=gemini-app', 'main', false],
    ['/accounts?pool=gemini-app', undefined, false],
    ['/accounts?mini=0', undefined, false],
    ['/accounts?mini=1', 'main', false],
    ['/index.html?mini=1', undefined, true],
    ['/index.html?mini=1', 'mini', true],
    ['/index.html', 'mini', true],
];

for (const [path, label, expected] of cases) {
    const fakeWindow = {
        location: new URL(path, 'http://localhost'),
        __TAURI_INTERNALS__: label ? { metadata: { currentWindow: { label } } } : undefined,
        matchMedia: () => ({ matches: false }),
    };
    const description = `${path} (${label ?? 'browser'})`;

    test(`pre-paint bootstrap: ${description}`, () => {
        const classes = new Set();
        runInNewContext(bootScript, {
            window: fakeWindow,
            document: { documentElement: {
                classList: { add: name => classes.add(name), remove: name => classes.delete(name) },
                style: {},
                setAttribute: () => {},
            } },
            localStorage: { getItem: () => null },
            URLSearchParams,
            console: { error: (_message, error) => { throw error; } },
        });
        assert.equal(classes.has('mini-window'), expected);
    });

    test(`runtime detection: ${description}`, t => {
        const original = Object.getOwnPropertyDescriptor(globalThis, 'window');
        Object.defineProperty(globalThis, 'window', { configurable: true, value: fakeWindow });
        t.after(() => {
            if (original) Object.defineProperty(globalThis, 'window', original);
            else Reflect.deleteProperty(globalThis, 'window');
        });
        assert.equal(isMiniWindow(), expected);
    });
}

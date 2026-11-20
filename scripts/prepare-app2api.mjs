import { copyFile, cp, mkdtemp, mkdir, readFile, rename, rm, writeFile, chmod } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

// Pin the actual source archive, not a mutable tag or an install script.
const revision = 'f41036a47e7b58b6929a3f2901aa806189f1da81';
const archiveHash = '19439e67614101d1b9d5f06aab4692f7f7b19acb4e4a7569102575f2eb73cc53';
const root = fileURLToPath(new URL('../', import.meta.url));
const output = join(root, 'src-tauri/resources/app2api-runtime');
const nodeName = process.platform === 'win32' ? 'node.exe' : 'node';
const adapter = await readFile(new URL('app2api-embedded.mjs', import.meta.url));
const digest = bytes => createHash('sha256').update(bytes).digest('hex');
const stamp = JSON.stringify([revision, process.platform, process.arch, process.version, digest(adapter), digest(await readFile(fileURLToPath(import.meta.url)))]);
if (Number(process.versions.node.split('.')[0]) < 22) throw new Error('Node.js 22 or newer is required on the build machine.');

async function download(url) {
  const response = await fetch(url, { signal: AbortSignal.timeout(45000) });
  if (!response.ok) throw new Error(`Could not download bundled dependency (${response.status}).`);
  return Buffer.from(await response.arrayBuffer());
}

async function prepare() {
  const previous = await readFile(join(output, 'build.json'), 'utf8').catch(() => null);
  if (previous === stamp) return;
  await mkdir(dirname(output), { recursive: true });
  const temp = await mkdtemp(join(dirname(output), '.app2api-'));
  try {
    const archive = await download(`https://codeload.github.com/ShawTim/gemini-app2api/tar.gz/${revision}`);
    if (digest(archive) !== archiveHash) throw new Error('App2API source checksum mismatch.');
    const tar = join(temp, 'app2api.tar.gz');
    const prefix = `gemini-app2api-${revision}`;
    await writeFile(tar, archive);
    execFileSync('tar', ['-xzf', tar, '-C', temp, `${prefix}/src`, `${prefix}/native-wire`, `${prefix}/LICENSE`]);
    const bundle = join(temp, 'runtime');
    const upstream = join(bundle, 'upstream');
    await mkdir(upstream, { recursive: true });
    for (const part of ['src', 'native-wire']) await cp(join(temp, prefix, part), join(upstream, part), { recursive: true });
    await copyFile(join(temp, prefix, 'LICENSE'), join(upstream, 'LICENSE'));
    await writeFile(join(bundle, 'NODE-LICENSE'), await download(`https://raw.githubusercontent.com/nodejs/node/${process.version}/LICENSE`));
    await copyFile(process.execPath, join(bundle, nodeName));
    if (process.platform !== 'win32') await chmod(join(bundle, nodeName), 0o755);
    await writeFile(join(bundle, 'embedded.mjs'), adapter);

    // On Windows JS stat mode is synthesized and directory fsync is unsupported.
    // The parent locks the private app-data store; Windows uses the user's inherited ACL.
    const stateFile = join(upstream, 'src/gateway/state.mjs');
    let state = await readFile(stateFile, 'utf8');
    const modeCheck = '    st.mode & 0o077';
    const directorySync = '      const dir = fs.openSync(this.directory, "r");\n      try {\n        fs.fsyncSync(dir);\n      } finally {\n        fs.closeSync(dir);\n      }';
    if (!state.includes(modeCheck) || !state.includes(directorySync)) throw new Error('App2API state adaptation no longer matches its pinned source.');
    state = state.replace(modeCheck, '    (process.platform !== "win32" && (st.mode & 0o077))');
    state = state.replace(directorySync, `      if (process.platform !== "win32") {\n${directorySync}\n      }`);
    await writeFile(stateFile, state);
    // The upstream package provides its own public desktop client defaults. We do not
    // commit its source archive, generated defaults, or any user credentials to this repo.
    await writeFile(join(bundle, '.gitkeep'), '');
    await writeFile(join(bundle, 'build.json'), stamp);
    await rm(output, { recursive: true, force: true });
    await rename(bundle, output);
    console.log(`Prepared bundled App2API (${process.platform}/${process.arch}).`);
  } finally {
    await rm(temp, { recursive: true, force: true });
  }
}

await prepare();

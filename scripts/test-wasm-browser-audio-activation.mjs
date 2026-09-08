#!/usr/bin/env node

import fs from 'node:fs/promises';
import http from 'node:http';
import path from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';
import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);
const { chromium, firefox } = require('playwright');
const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const targetRoot = path.join(repoRoot, 'target');

function usage(message) {
  if (message) console.error(`error: ${message}`);
  console.error(
    'usage: NODE_PATH=<playwright node_modules> node scripts/test-wasm-browser-audio-activation.mjs (--candidate-url URL | --candidate-wasm-dir PATH) [--baseline-url URL | --baseline-wasm-dir PATH] [--browser chromium|firefox|edge] [--chromium-executable PATH] [--firefox-executable PATH] [--edge-executable PATH] [--output-dir PATH] [--gesture-only] [--require-firefox-baseline-suspended]',
  );
  process.exit(2);
}

function parseArgs(args) {
  const options = {
    candidateUrl: null, baselineUrl: null, candidateWasmDir: null, baselineWasmDir: null,
    chromiumExecutable: null, firefoxExecutable: null, edgeExecutable: null, browser: null, outputDir: null,
    gestureOnly: false, requireFirefoxBaselineSuspended: false,
  };
  const optionNames = new Map([
    ['--candidate-url', 'candidateUrl'], ['--baseline-url', 'baselineUrl'],
    ['--candidate-wasm-dir', 'candidateWasmDir'], ['--baseline-wasm-dir', 'baselineWasmDir'],
    ['--chromium-executable', 'chromiumExecutable'], ['--firefox-executable', 'firefoxExecutable'],
    ['--edge-executable', 'edgeExecutable'], ['--browser', 'browser'], ['--output-dir', 'outputDir'],
  ]);
  for (let index = 0; index < args.length; index += 1) {
    const arg = args[index];
    if (optionNames.has(arg)) {
      const value = args[index + 1];
      if (!value || value.startsWith('--')) usage(`missing value for ${arg}`);
      options[optionNames.get(arg)] = value;
      index += 1;
    } else if (arg === '--gesture-only') {
      options.gestureOnly = true;
    } else if (arg === '--require-firefox-baseline-suspended') {
      options.requireFirefoxBaselineSuspended = true;
    } else {
      usage(`unknown argument ${arg}`);
    }
  }
  if ((options.candidateUrl === null) === (options.candidateWasmDir === null)) {
    usage('provide exactly one candidate URL or WASM directory');
  }
  if (options.baselineUrl && options.baselineWasmDir) usage('baseline URL and WASM directory are mutually exclusive');
  if (options.browser && !['chromium', 'firefox', 'edge'].includes(options.browser)) usage('--browser must be chromium, firefox, or edge');
  if (options.requireFirefoxBaselineSuspended && !(options.baselineUrl || options.baselineWasmDir)) {
    usage('--require-firefox-baseline-suspended requires a baseline');
  }
  if (options.requireFirefoxBaselineSuspended && !options.gestureOnly) {
    usage('--require-firefox-baseline-suspended requires --gesture-only');
  }
  return options;
}

function assertDiagnostic(value) {
  const numeric = ['currentTime', 'scheduledSources', 'activationResumeAttempts', 'fallbackResumeAttempts'];
  if (!value || typeof value.contextState !== 'string' || numeric.some((field) => !Number.isFinite(value[field]))) {
    throw new Error(`bridge returned an invalid diagnostic: ${JSON.stringify(value)}`);
  }
  return value;
}

async function diagnostic(page) {
  return assertDiagnostic(await page.evaluate(() => window.__zeffAudioBrowserTest.diagnostic()));
}

function makeBridgePage() {
  return `<!doctype html>
<meta charset="utf-8">
<title>Zeff WASM audio activation test</title>
<style>body { min-height: 100vh; }</style>
<script>globalThis.zeffBoyBoot = { preflight: Promise.resolve(false) };</script>
<script type="module">
  import init, {
    zeffAudioBrowserTestSetup,
    zeffAudioBrowserTestDiagnostic,
  } from './zeff_audio_browser_test.js';
  await init();
  zeffAudioBrowserTestSetup();
  window.__zeffAudioBrowserTest = { diagnostic: () => zeffAudioBrowserTestDiagnostic() };
</script>`;
}

async function startBridgeServer(fixtures) {
  const server = http.createServer(async (request, response) => {
    try {
      const url = new URL(request.url, 'http://127.0.0.1');
      const [, fixtureName, ...rest] = url.pathname.split('/');
      const directory = fixtures.get(fixtureName);
      if (!directory) return response.writeHead(404).end();
      const relativePath = rest.join('/') || 'index.html';
      if (relativePath === 'index.html') {
        return response.writeHead(200, { 'content-type': 'text/html; charset=utf-8', 'cache-control': 'no-store' }).end(makeBridgePage());
      }
      const normalizedPath = path.posix.normalize(relativePath);
      if (normalizedPath.startsWith('../') || path.posix.isAbsolute(normalizedPath)) {
        return response.writeHead(404).end();
      }
      const filePath = path.resolve(directory, normalizedPath);
      if (!filePath.startsWith(`${path.resolve(directory)}${path.sep}`)) {
        return response.writeHead(404).end();
      }
      const bytes = await fs.readFile(filePath);
      return response.writeHead(200, {
        'content-type': normalizedPath.endsWith('.wasm') ? 'application/wasm' : 'text/javascript; charset=utf-8',
        'cache-control': 'no-store',
      }).end(bytes);
    } catch (error) {
      return response.writeHead(500, { 'content-type': 'text/plain; charset=utf-8' }).end(String(error));
    }
  });
  await new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', resolve);
  });
  const address = server.address();
  return {
    urlFor: (name) => `http://127.0.0.1:${address.port}/${name}/`,
    close: () => new Promise((resolve, reject) => server.close((error) => (error ? reject(error) : resolve()))),
  };
}

function candidatePassed(lane, gestureOnly) {
  const needsActivation = ['suspended', 'interrupted'].includes(lane.beforeClick.contextState);
  return lane.beforeClick.scheduledSources > 0
    && lane.final.contextState === 'running'
    && lane.final.currentTime > lane.beforeClick.currentTime
    && (gestureOnly
      ? lane.final.scheduledSources >= lane.beforeClick.scheduledSources
      : lane.final.scheduledSources > lane.beforeClick.scheduledSources)
    && (!needsActivation || lane.final.activationResumeAttempts > lane.beforeClick.activationResumeAttempts);
}

function firefoxBaselineStayedSilent(lane) {
  return lane?.status === 'observed'
    && lane.beforeClick.contextState === 'suspended'
    && lane.final.contextState === 'suspended'
    && lane.final.currentTime <= lane.beforeClick.currentTime;
}

async function runLane({ label, browserName, browserType, executablePath, url, outputDir, gestureOnly }) {
  const lane = { label, browser: browserName, executablePath, url, status: 'failed' };
  let browser;
  try {
    await fs.access(executablePath);
    browser = await browserType.launch({ executablePath, headless: true });
    lane.browserVersion = browser.version();
    const page = await browser.newPage();
    await page.goto(url, { waitUntil: 'networkidle', timeout: 30_000 });
    await page.waitForFunction(() => typeof window.__zeffAudioBrowserTest?.diagnostic === 'function', undefined, { timeout: 10_000 });
    await page.locator('#zeff-audio-activation').waitFor({ state: 'visible', timeout: 10_000 });
    lane.beforeClick = await diagnostic(page);
    if (gestureOnly) {
      // Keep the control from starting a source in its click handler.
      await page.mouse.click(300, 300);
    } else {
      await page.locator('#zeff-audio-activation').click({ timeout: 10_000 });
    }
    await page.waitForTimeout(750);
    lane.final = await diagnostic(page);
    lane.userActivation = await page.evaluate(() => ({
      hasBeenActive: navigator.userActivation?.hasBeenActive ?? null,
      isActive: navigator.userActivation?.isActive ?? null,
    }));
    lane.status = label === 'candidate' ? (candidatePassed(lane, gestureOnly) ? 'passed' : 'failed') : 'observed';
    if (label === 'candidate' && lane.status === 'failed') lane.error = 'candidate did not resume an already-scheduled audio source after trusted click';
  } catch (error) {
    lane.error = String(error?.stack ?? error);
    if (browser) {
      try {
        const page = browser.contexts()[0]?.pages()[0];
        if (page) await page.screenshot({ path: path.join(outputDir, `${label}-${browserName}-failure.png`) });
      } catch (screenshotError) {
        lane.screenshotError = String(screenshotError);
      }
    }
  } finally {
    if (browser) await browser.close();
  }
  return lane;
}

async function main() {
  const options = parseArgs(process.argv.slice(2));
  const stamp = new Date().toISOString().replaceAll(':', '').replaceAll('.', '');
  const outputDir = path.resolve(options.outputDir ?? path.join(targetRoot, 'wasm-browser-runs', `audio-activation-${stamp}`));
  try {
    await fs.mkdir(outputDir, { recursive: false });
  } catch (error) {
    if (error?.code === 'EEXIST') usage(`output directory already exists: ${outputDir}`);
    throw error;
  }

  const fixtures = new Map();
  if (options.candidateWasmDir) fixtures.set('candidate', path.resolve(options.candidateWasmDir));
  if (options.baselineWasmDir) fixtures.set('baseline', path.resolve(options.baselineWasmDir));
  const bridgeServer = fixtures.size ? await startBridgeServer(fixtures) : null;
  try {
    const candidateUrl = options.candidateUrl ?? bridgeServer.urlFor('candidate');
    const baselineUrl = options.baselineUrl ?? (options.baselineWasmDir ? bridgeServer.urlFor('baseline') : null);
    const browsers = [
      { browserName: 'chromium', browserType: chromium, executablePath: options.chromiumExecutable ?? chromium.executablePath() },
      { browserName: 'firefox', browserType: firefox, executablePath: options.firefoxExecutable ?? firefox.executablePath() },
    ];
    if (options.edgeExecutable) browsers.push({ browserName: 'edge', browserType: chromium, executablePath: options.edgeExecutable });
    const selectedBrowsers = options.browser ? browsers.filter((browser) => browser.browserName === options.browser) : browsers;
    if (!selectedBrowsers.length) usage('no executable configured for selected browser');

    const lanes = [];
    for (const browser of selectedBrowsers) {
      if (baselineUrl) lanes.push(await runLane({ label: 'baseline', url: baselineUrl, outputDir, gestureOnly: options.gestureOnly, ...browser }));
      lanes.push(await runLane({ label: 'candidate', url: candidateUrl, outputDir, gestureOnly: options.gestureOnly, ...browser }));
    }
    const firefoxBaseline = lanes.find((lane) => lane.label === 'baseline' && lane.browser === 'firefox');
    const report = {
      schema: 1,
      purpose: 'Trusted-gesture Web Audio graph regression gate; context state and source scheduling, not physical speaker output.',
      outputDir,
      candidateUrl, baselineUrl, gestureOnly: options.gestureOnly, lanes,
      firefoxBaselineControl: options.requireFirefoxBaselineSuspended ? firefoxBaselineStayedSilent(firefoxBaseline) : null,
    };
    await fs.writeFile(path.join(outputDir, 'report.json'), `${JSON.stringify(report, null, 2)}\n`);

    const candidateLanesPass = lanes.filter((lane) => lane.label === 'candidate').every((lane) => lane.status === 'passed');
    const baselineControlPasses = !options.requireFirefoxBaselineSuspended || report.firefoxBaselineControl;
    if (!candidateLanesPass || !baselineControlPasses) {
      console.error(JSON.stringify(report, null, 2));
      process.exitCode = 1;
    } else {
      console.log(JSON.stringify(report, null, 2));
    }
  } finally {
    if (bridgeServer) await bridgeServer.close();
  }
}

await main();



#!/usr/bin/env node
// chromaglyph launcher: locates this platform's prebuilt binary inside an
// @chromaglyph/cli-* optional dependency and runs it with argv passthrough.
// The CLI itself is the same Rust binary the GitHub Releases and
// `cargo install` ship — this wrapper adds no behavior.

const { spawn } = require('node:child_process')
const { accessSync } = require('node:fs')
const path = require('node:path')

// candidate platform packages in preference order. On Linux the glibc
// build comes first and musl (Alpine etc.) is the fallback — a successful
// require.resolve is the only dependable musl probe from JS.
const PLATFORMS = {
  darwin: { arm64: ['darwin-arm64'], x64: ['darwin-x64'] },
  linux: {
    arm64: ['linux-arm64-gnu', 'linux-arm64-musl'],
    x64: ['linux-x64-gnu', 'linux-x64-musl'],
  },
  win32: { x64: ['win32-x64-msvc'] },
}

function fail(message) {
  console.error(`chromaglyph: ${message}`)
  process.exit(1)
}

const candidates = PLATFORMS[process.platform]?.[process.arch]
if (!candidates) {
  fail(
    `no prebuilt binary for ${process.platform}-${process.arch}; ` +
      'build the CLI from source instead: cargo install chromaglyph',
  )
}

const suffix = process.platform === 'win32' ? '.exe' : ''
let bin = null
for (const platform of candidates) {
  try {
    const dir = path.dirname(
      require.resolve(`@chromaglyph/cli-${platform}/package.json`),
    )
    const candidate = path.join(dir, `chromaglyph${suffix}`)
    accessSync(candidate)
    bin = candidate
    break
  } catch {
    // not installed — npm skips optional deps whose os/cpu don't match;
    // on musl hosts the gnu candidate is absent by design, keep trying
  }
}
if (!bin) {
  const names = candidates.map((c) => `@chromaglyph/cli-${c}`).join(', ')
  fail(
    `the binary package for ${process.platform}-${process.arch} is missing ` +
      `(expected ${names}). A partial install or a custom registry layout ` +
      'can drop optional dependencies — try reinstalling with ' +
      '`npm install -g chromaglyph --force`, and if it persists: ' +
      'https://github.com/chromaglyph/chromaglyph/issues',
  )
}

const child = spawn(bin, process.argv.slice(2), { stdio: 'inherit' })
child.on('error', (err) => fail(err.message))
child.on('close', (code, signal) => {
  if (signal) {
    // re-raise so shells, npm scripts and && chains see the termination
    process.kill(process.pid, signal)
  } else {
    process.exit(code ?? 1)
  }
})
// forward the signals a wrapper process receives down to the binary
for (const signal of ['SIGINT', 'SIGTERM', 'SIGHUP']) {
  process.on(signal, () => child.kill(signal))
}

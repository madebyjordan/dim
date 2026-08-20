import { createReadStream, rmSync, mkdtempSync } from 'node:fs';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { spawn } from 'node:child_process';

const [chrome, initInput, segmentInput, mime] = process.argv.slice(2);
if (!chrome || !initInput || !segmentInput || !mime) {
  throw new Error('Usage: node media-source-probe.mjs CHROME INIT SEGMENT MIME');
}
const init = resolve(initInput);
const segment = resolve(segmentInput);
const profile = mkdtempSync(join(tmpdir(), 'eclipse-mse-probe-'));
const html = `<!doctype html><meta charset="utf-8"><video id="media" muted></video><pre id="result">starting</pre><script>
const result = document.querySelector('#result');
const media = document.querySelector('#media');
const mime = ${JSON.stringify(mime)};
const history = [];
const report = (state, detail = {}) => {
  history.push({state, readyState: media.readyState, networkState: media.networkState, error: media.error && {code: media.error.code, message: media.error.message}, ...detail});
  result.textContent = JSON.stringify({mime, supported: MediaSource.isTypeSupported(mime), history});
};
for (const name of ['error', 'stalled', 'waiting', 'loadedmetadata', 'canplay', 'playing']) media.addEventListener(name, () => report('media-' + name));
if (!MediaSource.isTypeSupported(mime)) report('unsupported');
else {
  const source = new MediaSource();
  media.src = URL.createObjectURL(source);
  source.addEventListener('sourceopen', async () => {
    const buffer = source.addSourceBuffer(mime);
    buffer.addEventListener('error', () => report('source-buffer-error'));
    const append = (bytes) => new Promise((resolve, reject) => {
      const done = () => { buffer.removeEventListener('updateend', done); resolve(); };
      buffer.addEventListener('updateend', done);
      try { buffer.appendBuffer(bytes); } catch (error) { reject(error); }
    });
    try {
      await append(await fetch('/init').then((response) => response.arrayBuffer()));
      report('init-appended', {ranges: Array.from({length: buffer.buffered.length}, (_, index) => [buffer.buffered.start(index), buffer.buffered.end(index)])});
      await append(await fetch('/segment').then((response) => response.arrayBuffer()));
      report('segment-appended', {ranges: Array.from({length: buffer.buffered.length}, (_, index) => [buffer.buffered.start(index), buffer.buffered.end(index)])});
      source.endOfStream();
      await media.play().catch((error) => report('play-rejected', {detail: String(error)}));
      setTimeout(() => report('playback-observed', {currentTime: media.currentTime, ranges: Array.from({length: buffer.buffered.length}, (_, index) => [buffer.buffered.start(index), buffer.buffered.end(index)])}), 1500);
    } catch (error) { report('append-threw', {detail: String(error), stack: error && error.stack}); }
  }, {once: true});
}
</script>`;

const server = createServer((request, response) => {
  if (request.url === '/') {
    response.setHeader('Content-Type', 'text/html');
    response.end(html);
    return;
  }
  const file = request.url === '/init' ? init : request.url === '/segment' ? segment : null;
  if (!file) {
    response.statusCode = 404;
    response.end();
    return;
  }
  response.setHeader('Content-Type', 'video/mp4');
  createReadStream(file).pipe(response);
});

await new Promise((resolvePromise) => server.listen(0, '127.0.0.1', resolvePromise));
const port = server.address().port;
let child;
try {
  child = spawn(chrome, [
    '--headless=new',
    '--disable-gpu',
    '--no-first-run',
    '--autoplay-policy=no-user-gesture-required',
    `--user-data-dir=${profile}`,
    '--virtual-time-budget=5000',
    '--dump-dom',
    `http://127.0.0.1:${port}/`
  ], { stdio: ['ignore', 'pipe', 'pipe'] });
  let stdout = '';
  let stderr = '';
  child.stdout.setEncoding('utf8').on('data', (chunk) => { stdout += chunk; });
  child.stderr.setEncoding('utf8').on('data', (chunk) => { stderr += chunk; });
  const exit = await new Promise((resolvePromise) => child.on('exit', (code, signal) => resolvePromise({code, signal})));
  const match = stdout.match(/<pre id="result">([^<]+)<\/pre>/);
  console.log(match?.[1].replaceAll('&quot;', '"').replaceAll('&amp;', '&') ?? stdout);
  if (exit.code !== 0) {
    console.error(stderr);
    process.exitCode = exit.code ?? 1;
  }
} finally {
  if (child && child.exitCode === null) child.kill();
  await new Promise((resolvePromise) => server.close(resolvePromise));
  rmSync(profile, {recursive: true, force: true});
}

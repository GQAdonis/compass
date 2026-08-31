// Installed-product integration checks for Phase 3 only. No external services.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawn, spawnSync } from 'node:child_process';
import { createInterface } from 'node:readline';

const binary = process.env.COMPASS_TEST_BINARY || 'compass';
const fixture = fileURLToPath(new URL('../fixtures/compatibility/compass-0.3.6/', import.meta.url));
const root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'compass-legacy-integration-')));
const project = path.join(root, 'project with spaces');
const output = path.join(project, 'compass-out');
fs.mkdirSync(output, { recursive: true });
fs.copyFileSync(path.join(fixture, 'source/lib.rs'), path.join(project, 'lib.rs'));
const graph = path.join(output, 'graph.json');
fs.copyFileSync(path.join(fixture, 'graph.json'), graph);
const provenance = path.join(output, 'source-root.txt');
fs.writeFileSync(provenance, project + '\n');
const env = Object.fromEntries(Object.entries(process.env).filter(([key]) => !key.startsWith('COMPASS_')));
const expected = `compass update "${project}" --force`;
let checks = 0;
function pass(label) { checks += 1; console.log(`PASS ${label}`); }
function run(args, success = true) {
  const result = spawnSync(binary, args, { cwd: project, env, encoding: 'utf8', timeout: 60000, maxBuffer: 4 << 20 });
  assert.ifError(result.error);
  assert.equal(result.status === 0, success, `${args[0]}: ${result.stdout}\n${result.stderr}`);
  return result;
}
function legacy(text, recovery = expected) {
  assert.match(text, /Compass 0\.3\.6; minimum supported builder version is 0\.3\.23/);
  assert.ok(text.includes(recovery), text);
}
for (const args of [['ask', 'who calls callee?'], ['search', 'callee'], ['callers', 'callee'],
  ['callees', 'caller'], ['impact', 'callee'], ['explore', 'caller', 'callee'], ['node', 'caller', 'callee'],
  ['query', '--cql', 'MATCH (n:Function) RETURN n.name AS name']]) {
  const result = run([...args, '--graph', graph, '--engine', 'json', '--format', 'json'], false);
  legacy(result.stderr + result.stdout);
  pass(`legacy ${args[0]} rejects at load with exact recovery`);
}

async function mcp(expectLegacy) {
  const child = spawn(binary, ['serve', '--graph', graph, '--engine', 'json'], { cwd: project, env, stdio: ['pipe', 'pipe', 'pipe'] });
  const lines = createInterface({ input: child.stdout });
  const pending = new Map();
  let serial = 0, stderr = '';
  child.stderr.on('data', chunk => { stderr = (stderr + chunk).slice(-65536); });
  lines.on('line', line => {
    assert.ok(line.length <= (4 << 20));
    const message = JSON.parse(line);
    if (pending.has(message.id)) { pending.get(message.id)(message); pending.delete(message.id); }
  });
  async function rpc(method, params = {}) {
    const id = ++serial;
    params = { ...params, _meta: { 'io.modelcontextprotocol/protocolVersion': '2026-07-28', 'io.modelcontextprotocol/clientCapabilities': {} } };
    let timer;
    try {
      return await Promise.race([
        new Promise(resolve => { pending.set(id, resolve); child.stdin.write(JSON.stringify({ jsonrpc: '2.0', id, method, params }) + '\n'); }),
        new Promise((_, reject) => { timer = setTimeout(() => reject(new Error(`MCP deadline: ${stderr}`)), 20000); }),
      ]);
    } finally { clearTimeout(timer); pending.delete(id); }
  }
  try {
    const discovery = await rpc('server/discover');
    assert.ok(!discovery.error, JSON.stringify(discovery));
    const result = await rpc('tools/call', { name: 'search_symbols', arguments: { query: 'callee' } });
    if (expectLegacy) {
      assert.ok(result.error || result.result?.isError, JSON.stringify(result));
      legacy(JSON.stringify(result).replaceAll('\\"', '"'));
    } else {
      assert.ok(!result.error && !result.result?.isError, JSON.stringify(result));
      assert.ok(JSON.stringify(result).includes('callee'));
    }
    pass(expectLegacy ? 'MCP legacy load rejection' : 'MCP rebuilt artifact loads');
  } finally {
    child.stdin.end();
    const timer = setTimeout(() => child.kill('SIGTERM'), 2000);
    await new Promise(resolve => child.exitCode !== null ? resolve() : child.once('exit', resolve));
    clearTimeout(timer); lines.close();
  }
}
await mcp(true);
fs.unlinkSync(provenance);
const absent = run(['search', 'callee', '--graph', graph, '--engine', 'json'], false);
legacy(absent.stderr + absent.stdout, 'compass update "<source-root>" --force');
assert.match(absent.stderr + absent.stdout, /supply the project root explicitly/);
pass('missing provenance requires caller root');
fs.writeFileSync(provenance, project + '\n');
// Execute exactly the recovery command, with arguments passed separately.
run(['update', project, '--force']);
pass('exact force-rebuild command succeeds over authentic legacy artifact');
const rebuilt = JSON.parse(fs.readFileSync(graph, 'utf8'));
assert.equal(rebuilt.graph.build.builderVersion, '0.3.23');
assert.equal(JSON.parse(run(['search', 'callee', '--format', 'json']).stdout).schema, 'compass.query/1');
pass('rebuilt default-engine typed query');
assert.ok(JSON.parse(run(['query', '--cql', 'MATCH (n:Function) RETURN n.name AS name', '--format', 'json']).stdout).rows.length > 0);
pass('rebuilt default-engine CompassQL');
await mcp(false);
console.log(`${checks} changed-path checks passed; fixture retained at ${root}`);

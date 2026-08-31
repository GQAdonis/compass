// Changed-surface public integration checks. Run only AFTER release installation.
// Requires an explicitly selected disposable-capable local server; credentials
// come from COMPASS_TEST_SURREAL_* and are never printed or written to fixtures.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';
import { spawn, spawnSync } from 'node:child_process';
import { createInterface } from 'node:readline';

const binary = process.env.COMPASS_TEST_BINARY || 'compass';
const endpoint = process.env.COMPASS_TEST_SURREAL_ENDPOINT;
assert.ok(endpoint, 'COMPASS_TEST_SURREAL_ENDPOINT must select the test server explicitly');
const root = fs.mkdtempSync(path.join(os.tmpdir(), 'compass-server-integration-'));
const project = path.join(root, 'project');
fs.mkdirSync(project);
fs.writeFileSync(path.join(project, 'lib.rs'), 'pub fn caller() { callee(); }\npub fn callee() {}\n');
const namespace = 'compass_validation';
const database = `verify_${crypto.randomBytes(8).toString('hex')}`;
const yaml = path.join(root, 'connection.yaml');
fs.writeFileSync(yaml, `store: surreal\nengine: remote\nendpoint: ${JSON.stringify(endpoint)}\nnamespace: ${namespace}\ndatabase: ${database}\n`);
const clean = Object.fromEntries(Object.entries(process.env).filter(([key]) => key !== 'COMPASS_OUT' && key !== 'COMPASS_STORE' && !key.startsWith('COMPASS_SURREAL_')));
const auth = { ...clean };
for (const key of ['USERNAME', 'PASSWORD', 'TOKEN', 'AUTH_LEVEL']) {
  if (process.env[`COMPASS_TEST_SURREAL_${key}`]) auth[`COMPASS_SURREAL_${key}`] = process.env[`COMPASS_TEST_SURREAL_${key}`];
}
let checks = 0;
function run(label, args, { env = auth, cwd = project, success = true, config = yaml } = {}) {
  const result = spawnSync(binary, [...args, '--surreal-config', config], { cwd, env, encoding: 'utf8', timeout: 45000, maxBuffer: 4 << 20 });
  assert.ifError(result.error);
  assert.equal(result.status === 0, success, `${label}: ${result.stdout}\n${result.stderr}`);
  checks += 1;
  console.log(`PASS ${label}`);
  return result;
}
function json(label, args, options) { return JSON.parse(run(label, [...args, '--format', 'json'], options).stdout); }
function current() { return path.join(project, 'compass-out', 'snapshots', fs.readFileSync(path.join(project, 'compass-out/current-snapshot'), 'utf8').trim()); }
run('remote YAML init', ['init', '.', '--yes']);
const ref = JSON.parse(fs.readFileSync(path.join(current(), 'surreal.ref')));
assert.equal(ref.engine, 'remote');
assert.equal(ref.database, database);
assert.equal(ref.namespace, namespace);
assert.ok(!fs.existsSync(path.join(project, 'compass-out/surreal')), 'remote must not open server files');
for (const args of [['ask', 'who calls callee?'], ['search', 'callee'], ['callers', 'callee'], ['callees', 'caller'], ['impact', 'callee'], ['explore', 'caller', 'callee'], ['node', 'caller', 'callee']]) {
  assert.equal(json(`remote typed ${args[0]}`, [...args, '--engine', 'surreal']).schema, 'compass.query/1');
}
assert.equal(json('default selects remote reference', ['search', 'callee']).schema, 'compass.query/1');
const cql = 'PROFILE MATCH (n:Function) RETURN n.name AS name ORDER BY name';
const rows = json('remote native CompassQL', ['query', '--cql', cql, '--engine', 'surreal']);
assert.ok(rows.rows.length && rows.profile);
const unchanged = run('remote unchanged publication', ['update', '.']);
assert.match(unchanged.stdout, /\(0 extracted,/);
const shadowYaml = path.join(root, 'shadow.yaml');
fs.writeFileSync(shadowYaml, fs.readFileSync(yaml, 'utf8').replace(`database: ${database}`, 'database: wrong_yaml_database'));
assert.equal(json('environment overrides conflicting YAML', ['search', 'callee'], { env: { ...auth, COMPASS_SURREAL_DATABASE: database }, config: shadowYaml }).schema, 'compass.query/1');
assert.equal(json('flags override conflicting environment', ['search', 'callee', '--surreal-database', database], { env: { ...auth, COMPASS_SURREAL_DATABASE: 'not_the_snapshot' } }).schema, 'compass.query/1');
run('target mismatch fails closed', ['search', 'callee', '--surreal-database', 'not_the_snapshot'], { success: false });
if (auth.COMPASS_SURREAL_PASSWORD) {
  const denied = run('bad authentication fails closed', ['search', 'callee'], { env: { ...auth, COMPASS_SURREAL_PASSWORD: 'fixture-invalid-secret' }, success: false });
  assert.ok(!denied.stderr.includes('fixture-invalid-secret'));
  const passwordYaml = path.join(root, 'password-shadow.yaml');
  fs.writeFileSync(passwordYaml, fs.readFileSync(yaml, 'utf8') + 'password: fixture-invalid-secret\n');
  const selectorAuth = { ...auth, COMPASS_SURREAL_PASSWORD_ENV: 'COMPASS_TEST_SURREAL_PASSWORD' };
  delete selectorAuth.COMPASS_SURREAL_PASSWORD;
  assert.equal(json('environment credential selector overrides YAML password', ['search', 'callee'], { config: passwordYaml, env: selectorAuth }).schema, 'compass.query/1');
  assert.equal(json('flag credential selector overrides environment password', ['search', 'callee', '--surreal-password-env', 'COMPASS_TEST_SURREAL_PASSWORD'], { env: { ...auth, COMPASS_SURREAL_PASSWORD: 'fixture-invalid-secret' } }).schema, 'compass.query/1');
  const missingSelector = { ...auth };
  delete missingSelector.COMPASS_TEST_MISSING_PASSWORD;
  const missing = run('missing selected credential never falls back', ['search', 'callee', '--surreal-password-env', 'COMPASS_TEST_MISSING_PASSWORD'], { env: missingSelector, success: false });
  assert.match(missing.stderr, /credential environment variable is missing/);
}
const conflict = run('explicit storage flags reject conflicting engine', ['update', '.', '--store', 'json', '--surreal-engine', 'remote'], { success: false });
assert.match(conflict.stderr, /conflicting explicit flags/);
const backup = path.join(root, 'backup');
const restored = path.join(root, 'restored');
json('remote portable backup', ['store', 'backup', 'compass-out', '--output', backup]);
const restoredDatabase = `${database}_restored`;
json('remote restore to explicit new database', ['store', 'restore', '--from', backup, '--into', restored, '--surreal-database', restoredDatabase]);
assert.equal(json('remote restored validation', ['store', 'validate', restored, '--surreal-database', restoredDatabase]).valid, true);
fs.unlinkSync(path.join(restored, 'graph.json'));
assert.equal(json('remote typed query without JSON', ['search', 'callee', '--graph', path.join(restored, 'graph.json'), '--surreal-database', restoredDatabase]).schema, 'compass.query/1');
assert.ok(json('remote CompassQL without JSON', ['query', '--cql', cql, '--graph', path.join(restored, 'graph.json'), '--surreal-database', restoredDatabase]).rows.length);
const embedded = path.join(root, 'embedded-restore');
json('remote bundle restored embedded', ['store', 'restore', '--from', backup, '--into', embedded, '--surreal-engine', 'surrealkv']);
assert.equal(json('embedded restore validation', ['store', 'validate', embedded, '--surreal-engine', 'surrealkv']).valid, true);
assert.equal(json('embedded query retained', ['search', 'callee', '--graph', path.join(embedded, 'graph.json'), '--surreal-engine', 'surrealkv']).schema, 'compass.query/1');

// Real MCP stdio transport, exact generation reference, and the same cache.
const child = spawn(binary, ['serve', '--engine', 'surreal', '--surreal-config', yaml], { cwd: project, env: auth, stdio: ['pipe', 'pipe', 'pipe'] });
const lines = createInterface({ input: child.stdout });
const pending = new Map(); let serial = 0; let stderr = '';
child.stderr.on('data', chunk => { stderr = (stderr + chunk).slice(-65536); });
lines.on('line', line => {
  assert.ok(line.length <= (4 << 20), 'bounded MCP response');
  const message = JSON.parse(line);
  if (pending.has(message.id)) { pending.get(message.id)(message); pending.delete(message.id); }
});
async function rpc(method, params = {}) {
  params = { ...params, _meta: { 'io.modelcontextprotocol/protocolVersion': '2026-07-28', 'io.modelcontextprotocol/clientCapabilities': {} } };
  const id = ++serial;
  let timer;
  try {
    const response = await Promise.race([
      new Promise(resolve => { pending.set(id, resolve); child.stdin.write(JSON.stringify({ jsonrpc: '2.0', id, method, params }) + '\n'); }),
      new Promise((_, reject) => { timer = setTimeout(() => reject(new Error(`MCP ${method} deadline; ${stderr}`)), 20000); }),
    ]);
    assert.ok(!response.error, JSON.stringify(response.error));
    return response.result;
  } finally { clearTimeout(timer); pending.delete(id); }
}
try {
  await rpc('server/discover');
  const tools = await rpc('tools/list');
  assert.ok(tools.tools.some(t => t.name === 'search_symbols'));
  const result = await rpc('tools/call', { name: 'search_symbols', arguments: { query: 'callee' } });
  assert.ok(!result.isError, JSON.stringify(result));
  assert.ok(JSON.stringify(result).includes('callee'));
  checks += 1; console.log('PASS remote MCP discovery and typed query');
} finally {
  child.stdin.end();
  const timer = setTimeout(() => child.kill('SIGTERM'), 2000);
  await new Promise(resolve => child.exitCode !== null ? resolve() : child.once('exit', resolve));
  clearTimeout(timer); lines.close();
}
// Flags precede the explicit root, with no YAML endpoint: persisted project
// settings must be loaded from that root, not from the unrelated working dir.
const watchEnv = { ...auth, COMPASS_SURREAL_CONFIG: path.join(root, 'empty.yaml') };
fs.writeFileSync(watchEnv.COMPASS_SURREAL_CONFIG, '{}\n');
const watcher = spawn(binary, ['watch', '--poll', project], { cwd: root, env: watchEnv, stdio: ['ignore', 'pipe', 'pipe'] });
let watchOutput = '';
watcher.stdout.on('data', chunk => { watchOutput = (watchOutput + chunk).slice(-65536); });
watcher.stderr.on('data', chunk => { watchOutput = (watchOutput + chunk).slice(-65536); });
async function waitFor(predicate) {
  const deadline = Date.now() + 45000;
  while (!predicate()) {
    assert.equal(watcher.exitCode, null, watchOutput);
    assert.ok(Date.now() < deadline, `watch deadline: ${watchOutput}`);
    await new Promise(resolve => setTimeout(resolve, 250));
  }
}
try {
  await waitFor(() => /Watching( for changes| )/.test(watchOutput));
  const previous = JSON.parse(fs.readFileSync(path.join(current(), 'surreal.ref'))).generationId;
  fs.appendFileSync(path.join(project, 'lib.rs'), 'pub fn watched_change() {}\n');
  await waitFor(() => {
    try { return JSON.parse(fs.readFileSync(path.join(current(), 'surreal.ref'))).generationId !== previous; } catch { return false; }
  });
  checks += 1; console.log('PASS remote watch persists a changed generation');
} finally {
  watcher.kill('SIGINT');
  const timer = setTimeout(() => watcher.kill('SIGTERM'), 2000);
  await new Promise(resolve => watcher.exitCode !== null ? resolve() : watcher.once('exit', resolve));
  clearTimeout(timer);
}
assert.equal(json('reopen watched generation', ['search', 'watched_change']).schema, 'compass.query/1');
console.log(JSON.stringify({ status: 'PASS', checks, root, namespace, databases: [database, restoredDatabase], note: 'Only fresh validation databases were written; artifacts retained for inspection.' }));

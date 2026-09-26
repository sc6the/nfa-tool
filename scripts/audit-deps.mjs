// Metadata-only audit: does not compile or execute dependencies.
import { readFile } from 'node:fs/promises';
const lock = await readFile(new URL('../Cargo.lock', import.meta.url), 'utf8');
const packages = lock.split('[[package]]').slice(1).map(block => ({
  name: block.match(/^name = "([^"]+)"/m)?.[1],
  version: block.match(/^version = "([^"]+)"/m)?.[1],
  source: block.match(/^source = "([^"]+)"/m)?.[1],
})).filter(p => p.source?.startsWith('registry+'));
const response = await fetch('https://api.osv.dev/v1/querybatch', {
  method: 'POST', headers: { 'Content-Type': 'application/json' },
  body: JSON.stringify({ queries: packages.map(p => ({ package: { name: p.name, ecosystem: 'crates.io' }, version: p.version })) }),
});
if (!response.ok) throw new Error(`Advisory lookup failed: ${response.status}`);
const { results } = await response.json();
const findings = results.flatMap((r, i) => (r.vulns || []).map(v => ({ package: packages[i].name, version: packages[i].version, advisory: v.id })));
console.log(JSON.stringify({ checked: packages.length, findings }, null, 2));
process.exitCode = findings.length ? 1 : 0;

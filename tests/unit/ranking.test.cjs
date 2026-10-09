#!/usr/bin/env node
'use strict';
// E17 regression: pure ranking policy; optional isolated headless Chromium check.
// No Jelly service, CDP endpoint, persistent browser profile or package install.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const vm = require('node:vm');
const {spawnSync} = require('node:child_process');

const root = path.resolve(__dirname, '../..');
const rankingSource = fs.readFileSync(path.join(root, 'src/core/ranking.js'), 'utf8');
const ranking = vm.runInNewContext(rankingSource);
const toLocal = value => JSON.parse(JSON.stringify(value));
const expectedTier = (name, query) => name === query ? 0 : name.startsWith(query) ? 1 : name.includes(query) ? 2 : -1;
const oldComparator = (a, b) => a.match - b.match || a.disabled - b.disabled || a.offscreen - b.offscreen || a.order - b.order;
const oldActionability = (a, b) => a.disabled - b.disabled || a.offscreen - b.offscreen || a.order - b.order;
let checks = 0;
const check = (ok, message) => { assert.ok(ok, message); checks++; };

for (const query of ['', 'save', 'action', 'text', '💾', 'unknown', 'é']) {
    for (const name of ['', 'save', 'save draft', 'please save now', 'duplicate action', 'text entry', '💾 Save', 'étiquette']) {
        check(ranking.matchQuality(name, query) === expectedTier(name, query), `${name} / ${query}`);
    }
}
check(Object.isFrozen(ranking), 'ranking helpers are immutable');

let seed = 0x12345678;
const rand = () => ((seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0) / 4294967296);
const names = ['save', 'save draft', 'autosave', 'save file', 'cancel', ' SAVE', ''];
for (let trial = 0; trial < 60; trial++) {
    const rows = [];
    for (let order = 0; order < 250; order++) {
        const name = names[Math.floor(rand() * names.length)];
        const match = ranking.matchQuality(name, 'save');
        if (match === -1) continue;
        rows.push({match, disabled: +(rand() < 0.27), offscreen: +(rand() < 0.45), order, value: order});
    }
    const actual = rows.slice().sort(ranking.compareSearch).map(r => r.value);
    const baseline = rows.slice().sort(oldComparator).map(r => r.value);
    check(JSON.stringify(actual) === JSON.stringify(baseline), `sorted search parity trial ${trial}`);
    const action = rows.slice().sort(ranking.compareActionability).map(r => r.value);
    const legacy = rows.slice().sort(oldActionability).map(r => r.value);
    check(JSON.stringify(action) === JSON.stringify(legacy), `resolveText comparator parity trial ${trial}`);
    const first = actual.slice(0, 13);
    const second = actual.slice(13, 26);
    check(JSON.stringify(first.concat(second)) === JSON.stringify(actual.slice(0, 26)), 'pagination stable');
}

const tierRows = [
    {match: 2, disabled: 0, offscreen: 0, order: 0},
    {match: 0, disabled: 1, offscreen: 0, order: 1},
    {match: 0, disabled: 0, offscreen: 1, order: 2},
    {match: 0, disabled: 0, offscreen: 0, order: 3},
    {match: 1, disabled: 0, offscreen: 0, order: 4},
];
check(JSON.stringify(tierRows.map((r, i) => ({...r, id: i})).sort(ranking.compareSearch).map(r => r.id)) === '[3,2,1,4,0]', 'exact > enabled > viewport > prefix > contains');
console.log(`PASS: ${checks} pure ranking parity assertions (60 seeded ranking fixtures)`);

if (!process.argv.includes('--browser')) process.exit(0);

const runtimeRust = fs.readFileSync(path.join(root, 'src/browser/runtime.rs'), 'utf8');
function raw(name) {
    const match = runtimeRust.match(new RegExp(`(?:const|pub\\(super\\) const) ${name}: &str = r#"([\\s\\S]*?)"#;`));
    check(match, `${name} raw string exists`);
    return match[1];
}
const stringConst = name => {
    const escaped = runtimeRust.match(new RegExp(`const ${name}: &str = ("(?:[^"\\\\]|\\\\.)*");`));
    check(escaped, `${name} string exists`);
    return JSON.parse(escaped[1]);
};
const roles = stringConst('INTERACTIVE_ROLES_SOURCE');
const selector = stringConst('INTERACTIVE_SELECTOR_SOURCE');
const normalize = raw('NORMALIZE_SOURCE');
const isInteractive = raw('IS_INTERACTIVE_SOURCE');
const measure = raw('MEASURE_SOURCE');
const inferRole = raw('INFER_ROLE_SOURCE');
const inViewport = raw('IN_VIEWPORT_SOURCE');
const version = Number(runtimeRust.match(/PAGE_RUNTIME_VERSION: u32 = (\d+);/)[1]);
const substitutions = {
    __JELLY_RUNTIME_VERSION__: String(version),
    __JELLY_INTERACTIVE_ROLES__: roles,
    __JELLY_INTERACTIVE_SELECTOR__: selector,
    __JELLY_NORMALIZE_FN__: normalize,
    __JELLY_IS_INTERACTIVE_FN__: isInteractive,
    __JELLY_MEASURE_FN__: measure,
    __JELLY_INFER_ROLE_FN__: inferRole,
    __JELLY_IN_VIEWPORT_FN__: inViewport,
    __JELLY_RANKING_FN__: rankingSource,
};
let runtime = raw('PAGE_RUNTIME_TEMPLATE');
for (const [key, value] of Object.entries(substitutions)) runtime = runtime.replace(key, value);
check(!runtime.includes('__JELLY_'), 'all runtime placeholders replaced');
new vm.Script(runtime); // syntax check of assembled browser runtime.

const legacyMatch = runtimeRust.slice(runtimeRust.indexOf('pub fn legacy_search_expression(')).match(/r#"([\s\S]*?)"#,/);
check(legacyMatch, 'legacy search expression exists');
function legacy(query, limit, offset) {
    const params = {query: JSON.stringify(query), limit, offset, roles, selector, normalize,
        is_interactive: isInteractive, measure, infer_role: inferRole, in_viewport: inViewport, ranking: rankingSource};
    let body = legacyMatch[1];
    for (const [key, value] of Object.entries(params)) body = body.replaceAll(`{${key}}`, String(value));
    body = body.replaceAll('{{', '{').replaceAll('}}', '}');
    new vm.Script(body);
    return body;
}

// Frozen pre-migration runtime: protects the comparison from later Git commits.
// The committed fixture was assembled from ddced5d:src/browser/runtime.rs.
const baselineRuntime = fs.readFileSync(path.join(root, 'tests/fixtures/page-runtime-v19.js'), 'utf8');
const baselineVersion = 19;
new vm.Script(baselineRuntime);

const fixtures = [
    {file: 'semantic-targets.html', check: `
        const runtime = globalThis.__jellyRuntimeV1;
        const idOf = row => runtime.refs.get(row.ref.slice(1))?.id;
        const search = (q, n = 10, o = 0) => runtime.search(q, n, o).map(idOf);
        const legacy = (q, n = 10, o = 0) => {
            const rows = (0,eval)(window.__jellyLegacySearchSource(q, n, o));
            return rows.map(row => document.querySelector('[data-jelly-ref="' + row.ref.slice(1) + '"]')?.id);
        };
        const queries = ['Save','Duplicate action','Viewport action','Priority action','Missing target'];
        for (const q of queries) {
            const a = search(q), b = legacy(q);
            if (JSON.stringify(a) !== JSON.stringify(b)) throw Error('runtime vs legacy mismatch ' + q + ': ' + JSON.stringify({a,b}));
        }
        if (idOf(runtime.search('Duplicate action',1,0)[0]) !== 'duplicate-enabled') throw Error('duplicate enabled');
        if (idOf(runtime.search('Priority action',1,0)[0]) !== 'priority-enabled-offscreen') throw Error('priority enabled');
        if (idOf(runtime.search('Viewport action',1,0)[0]) !== 'onscreen-duplicate') throw Error('viewport precedence');
        if (JSON.stringify(search('Save').slice(0,3)) !== JSON.stringify(['rank-exact','rank-prefix','rank-contains'])) throw Error('tier precedence');
        if (runtime.resolveText('Duplicate action')?.id !== 'duplicate-enabled') throw Error('resolveText actionability');
        for (const q of queries) {
            const all = search(q,20,0);
            if (JSON.stringify(search(q,2,0).concat(search(q,2,2))) !== JSON.stringify(all.slice(0,4))) throw Error('pagination ' + q);
        }
        const firstRef = runtime.search('Save',1,0)[0].ref;
        document.getElementById('rank-exact').setAttribute('aria-label','Changed');
        runtime.markDirty();
        if (!runtime.search('Changed',5).some(row=>row.ref === firstRef)) throw Error('stable ref on mutation');
        if (runtime.search('Save',10).some(row=>row.ref === firstRef)) throw Error('stale name after mutation');
        return {searchQueries:queries.length, firstRefStable:true, version:runtime.version};
    `},
    {file: 'browser-perf.html', check: `
        window.__jellyBench.setCase('large');
        (0,eval)(${JSON.stringify(baselineRuntime)});
        const old = globalThis.__jellyRuntimeV1;
        if (old.version !== ${baselineVersion}) throw Error('unexpected baseline version');
        const observe = rt => rt.search('large', 40, 0).map(r => [r.tag,r.role,r.name,r.disabled,r.in_viewport,r.shadow]);
        const baselineRows = observe(old);
        const measure = rt => {
            for (let i = 0; i < 3; i++) observe(rt);
            const samples = [];
            for (let i = 0; i < 12; i++) {
                const start = performance.now();
                observe(rt);
                samples.push(performance.now() - start);
            }
            samples.sort((a,b) => a-b);
            return Math.round(samples[6] * 1000) / 1000;
        };
        const oldMedianMs = measure(old);
        (0,eval)(${JSON.stringify(runtime)});
        const latest = globalThis.__jellyRuntimeV1;
        if (latest.version !== ${version}) throw Error('unexpected new version');
        const newRows = observe(latest);
        if (JSON.stringify(baselineRows) !== JSON.stringify(newRows)) throw Error('large fixture search parity regression');
        const newMedianMs = measure(latest);
        if (newRows.length !== 40) throw Error('large result count regression');
        return {controls: latest.cache.length, rows: newRows.length,
            baselineVersion:${baselineVersion}, newVersion:${version}, oldMedianMs, newMedianMs,
            note:'single isolated-browser sample; not a performance regression threshold'};
    `},
    {file: 'shadow-targets.html', check: `
        const runtime = globalThis.__jellyRuntimeV1;
        const refRows = runtime.search('Shadow click', 10).filter(row => row.name === 'Shadow click');
        if (refRows.length !== 1 || !refRows[0].shadow) throw Error('missing open shadow');
        const nested = runtime.search('Nested shadow click', 10);
        if (nested.length !== 1 || !nested[0].shadow) throw Error('missing nested shadow');
        if (runtime.search('Closed shadow action',10).length) throw Error('exposed closed shadow');
        return {openShadow:true, nestedShadow:true, closedShadowHidden:true};
    `},
];

const temp = fs.mkdtempSync(path.join(os.tmpdir(), 'jelly-e17-isolated-'));
try {
    for (const {file, check: code} of fixtures) {
        let fixture = fs.readFileSync(path.join(root, 'tests/fixtures', file), 'utf8');
        // Prevent literal closing script tags in injected code; no external dependencies.
        const stub = `<script>\ntry {\n` + runtime + `\n` +
            `window.__jellyLegacySearchSource = (q,n,o) => { ` +
            `return ${JSON.stringify(legacy('Save',10,0))}.replace('const query = "Save";', 'const query = ' + JSON.stringify(q) + ';').replace('const max = 10;', 'const max = '+n+';').replace('const offset = 0;', 'const offset = '+o+';'); };\n` +
            `const result = (() => { ${code} })();\n` +
            `document.body.setAttribute('data-e17-result', btoa(JSON.stringify({ok:true, result})));\n` +
            `} catch(e) { document.body.setAttribute('data-e17-result', btoa(JSON.stringify({ok:false, error:String(e), stack:e.stack}))); }\n</script>`;
        fixture = fixture.replace('</body>', stub + '\n</body>');
        const filename = path.join(temp, file);
        fs.writeFileSync(filename, fixture);
        const profile = path.join(temp, 'profile-' + file);
        const args = [
            '--headless=new','--no-sandbox','--disable-gpu','--disable-dev-shm-usage',
            '--disable-background-networking','--disable-sync','--no-first-run',
            `--user-data-dir=${profile}`, '--window-size=1280,800','--dump-dom', `file://${filename}`
        ];
        const res = spawnSync(process.env.JELLY_TEST_CHROMIUM || '/usr/bin/chromium', args, {encoding:'utf8', timeout:25000, maxBuffer:5e6});
        if (res.error) throw res.error;
        const encoded = res.stdout.match(/data-e17-result="([^"]+)"/);
        assert.ok(encoded, `Chromium DOM result for ${file}, exit=${res.status}, stderr=${res.stderr.slice(-1200)}`);
        const testResult = JSON.parse(Buffer.from(encoded[1], 'base64').toString('utf8'));
        assert.ok(testResult.ok, `${file} browser failure: ${JSON.stringify(testResult)}`);
        console.log(`PASS: isolated Chromium ${file} ${JSON.stringify(toLocal(testResult.result))}`);
    }
} finally {
    fs.rmSync(temp, {recursive:true, force:true});
}

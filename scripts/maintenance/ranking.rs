//! Real Chromium execution, without Node.js or a JavaScript host outside the browser.
use super::Result;
use serde_json::json;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant},
};
static NONCE: AtomicU64 = AtomicU64::new(0);
fn raw(source: &str, name: &str) -> Result<String> {
    let header = format!("{name}: &str = r#\"");
    let (_, tail) = source
        .split_once(&header)
        .ok_or_else(|| format!("missing raw Rust constant {name}"))?;
    Ok(tail
        .split_once("\"#;")
        .ok_or("unterminated raw Rust constant")?
        .0
        .to_owned())
}
fn string_constant(source: &str, name: &str) -> Result<String> {
    let (_, tail) = source
        .split_once(&format!("{name}: &str = "))
        .ok_or("missing quoted constant")?;
    let line = tail.lines().next().ok_or("missing value")?;
    Ok(serde_json::from_str::<String>(line.trim_end_matches(';'))?)
}
fn replace(template: &str, replacements: &[(&str, String)]) -> String {
    let mut result = template.to_owned();
    for (key, val) in replacements {
        result = result.replacen(key, val, 1)
    }
    result
}
fn runtime_parts(root: &Path) -> Result<(String, String)> {
    let source = fs::read_to_string(root.join("src/browser/runtime.rs"))?;
    let ranking = fs::read_to_string(root.join("src/core/ranking.js"))?;
    let roles = string_constant(&source, "INTERACTIVE_ROLES_SOURCE")?;
    let selector = string_constant(&source, "INTERACTIVE_SELECTOR_SOURCE")?;
    let normalize = raw(&source, "NORMALIZE_SOURCE")?;
    let interactive = raw(&source, "IS_INTERACTIVE_SOURCE")?;
    let measure = raw(&source, "MEASURE_SOURCE")?;
    let infer = raw(&source, "INFER_ROLE_SOURCE")?;
    let viewport = raw(&source, "IN_VIEWPORT_SOURCE")?;
    let version = source
        .split_once("PAGE_RUNTIME_VERSION: u32 = ")
        .ok_or("runtime version missing")?
        .1
        .split_once(';')
        .ok_or("version terminator missing")?
        .0;
    let runtime = replace(
        &raw(&source, "PAGE_RUNTIME_TEMPLATE")?,
        &[
            ("__JELLY_RUNTIME_VERSION__", version.to_owned()),
            ("__JELLY_INTERACTIVE_ROLES__", roles.clone()),
            ("__JELLY_INTERACTIVE_SELECTOR__", selector.clone()),
            ("__JELLY_NORMALIZE_FN__", normalize.clone()),
            ("__JELLY_IS_INTERACTIVE_FN__", interactive.clone()),
            ("__JELLY_MEASURE_FN__", measure.clone()),
            ("__JELLY_INFER_ROLE_FN__", infer.clone()),
            ("__JELLY_IN_VIEWPORT_FN__", viewport.clone()),
            ("__JELLY_RANKING_FN__", ranking.clone()),
        ],
    );
    if runtime.contains("__JELLY_") {
        return Err("unreplaced page runtime marker".into());
    }
    let (_, tail) = source
        .split_once("pub fn legacy_search_expression(")
        .ok_or("legacy search missing")?;
    let legacy = tail
        .split_once("r#\"")
        .ok_or("legacy raw missing")?
        .1
        .split_once("\"#,")
        .ok_or("legacy raw terminator missing")?
        .0;
    let legacy = replace(
        legacy,
        &[
            ("{query}", "\"Save\"".into()),
            ("{limit}", "10".into()),
            ("{offset}", "0".into()),
            ("{roles}", roles),
            ("{selector}", selector),
            ("{normalize}", normalize),
            ("{is_interactive}", interactive),
            ("{measure}", measure),
            ("{infer_role}", infer),
            ("{in_viewport}", viewport),
            ("{ranking}", ranking),
        ],
    )
    .replace("{{", "{")
    .replace("}}", "}");
    Ok((runtime, legacy))
}
struct Cleanup(PathBuf);
impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn execute(chromium: &str, html: &str, temp: &Path, label: &str) -> Result<String> {
    let file = temp.join(format!("{label}.html"));
    fs::write(&file, html)?;
    let profile = temp.join(format!("profile-{label}"));
    let output_path = temp.join(format!("{label}.dump"));
    let mut child = Command::new(chromium)
        .args([
            "--headless=new",
            "--no-sandbox",
            "--disable-gpu",
            "--disable-dev-shm-usage",
            "--disable-background-networking",
            "--disable-sync",
            "--no-first-run",
        ])
        .arg(format!("--user-data-dir={}", profile.display()))
        .arg("--window-size=1280,800")
        .arg("--dump-dom")
        .arg(format!("file://{}", file.display()))
        .stdout(Stdio::from(fs::File::create(&output_path)?))
        .stderr(Stdio::null())
        .spawn()?;
    let start = Instant::now();
    loop {
        if child.try_wait()?.is_some() {
            break;
        }
        if start.elapsed() > Duration::from_secs(25) {
            child.kill()?;
            let _ = child.wait();
            return Err(format!("Chromium test {label} timed out").into());
        }
        thread::sleep(Duration::from_millis(50));
    }
    let status = child.wait()?;
    let html = fs::read_to_string(&output_path)?;
    if !status.success() {
        return Err(format!("Chromium {label} exited {}", status).into());
    }
    if html.contains("data-e17-result=\"PASS\"") {
        Ok(label.to_owned())
    } else {
        let fault = html
            .split_once("data-e17-result=\"")
            .map(|(_, s)| s.split('"').next().unwrap_or("unknown"))
            .unwrap_or("no result attribute");
        Err(format!("{label}: browser assertions failed: {fault}").into())
    }
}
pub fn run(root: &Path) -> Result<()> {
    let dir = std::env::temp_dir().join(format!(
        "jelly-ranking-rust-{}-{}",
        std::process::id(),
        NONCE.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&dir)?;
    let _guard = Cleanup(dir.clone());
    let chromium =
        std::env::var("JELLY_TEST_CHROMIUM").unwrap_or_else(|_| "/usr/bin/chromium".into());
    let (runtime, legacy) = runtime_parts(root)?;
    let baseline = fs::read_to_string(root.join("tests/fixtures/page-runtime-v19.js"))?;
    let ranking = fs::read_to_string(root.join("src/core/ranking.js"))?;
    let pure = format!(
        r#"
const ranking={ranking};
const oldComparator=(a,b)=>a.match-b.match||a.disabled-b.disabled||a.offscreen-b.offscreen||a.order-b.order;
const oldActionability=(a,b)=>a.disabled-b.disabled||a.offscreen-b.offscreen||a.order-b.order;
const check=(p,msg)=>{{if(!p)throw Error(msg)}};
let checks=0;
for(const query of ['', 'save', 'action', 'text', '💾', 'unknown','é']){{
for(const name of ['', 'save','save draft','please save now','duplicate action','text entry','💾 Save','étiquette']){{
const tier=name===query?0:name.startsWith(query)?1:name.includes(query)?2:-1;
check(ranking.matchQuality(name,query)===tier,'tier '+query+' '+name);checks++;
}}}}
check(Object.isFrozen(ranking),'frozen');checks++;
let seed=0x12345678;const rand=()=>((seed=(Math.imul(seed,1664525)+1013904223)>>>0)/4294967296);
const names=['save','save draft','autosave','save file','cancel',' SAVE',''];
for(let trial=0;trial<60;trial++){{const rows=[];for(let order=0;order<250;order++){{
 const name=names[Math.floor(rand()*names.length)],match=ranking.matchQuality(name,'save');
 if(match<0)continue;
 rows.push({{match,disabled:+(rand()<0.27),offscreen:+(rand()<0.45),order,value:order}});
}}
const actual=rows.slice().sort(ranking.compareSearch).map(r=>r.value);
const expected=rows.slice().sort(oldComparator).map(r=>r.value);
check(JSON.stringify(actual)===JSON.stringify(expected),'search parity '+trial);checks++;
const action=rows.slice().sort(ranking.compareActionability).map(r=>r.value);
const old=rows.slice().sort(oldActionability).map(r=>r.value);
check(JSON.stringify(action)===JSON.stringify(old),'action parity '+trial);checks++;
check(JSON.stringify(actual.slice(0,13).concat(actual.slice(13,26)))===JSON.stringify(actual.slice(0,26)),'paging');checks++;
}}
const tierRows=[{{match:2,disabled:0,offscreen:0,order:0}},{{match:0,disabled:1,offscreen:0,order:1}},{{match:0,disabled:0,offscreen:1,order:2}},{{match:0,disabled:0,offscreen:0,order:3}},{{match:1,disabled:0,offscreen:0,order:4}}];
check(JSON.stringify(tierRows.map((r,i)=>({{...r,id:i}})).sort(ranking.compareSearch).map(r=>r.id))==='[3,2,1,4,0]','tier order');checks++;
check(checks===238,'pure assertion count '+checks);
"#
    );
    // Check helpers in an isolated document, evaluated exclusively by Chromium.
    let blank = format!(
        "<!doctype html><html><body><script>try{{{pure}document.body.setAttribute('data-e17-result','PASS')}}catch(e){{document.body.setAttribute('data-e17-result',String(e))}}</script></body></html>"
    );
    execute(&chromium, &blank, &dir, "pure-ranking")?;
    for fixture in [
        "semantic-targets.html",
        "shadow-targets.html",
        "browser-perf.html",
    ] {
        let original = fs::read_to_string(root.join("tests/fixtures").join(fixture))?;
        let assertions = match fixture {
            "semantic-targets.html" => {
                r#"
const rt=globalThis.__jellyRuntimeV1;
const get=row=>rt.refs.get(row.ref.slice(1))?.id;
const first=q=>get(rt.search(q,1,0)[0]);
for(const q of ['Save','Duplicate action','Viewport action','Priority action','Missing target']){
 const fast=rt.search(q,10,0).map(get);
 const expr=window.__legacy.replace('const query = "Save";','const query = '+JSON.stringify(q)+';');
 const slow=(0,eval)(expr).map(x=>document.querySelector('[data-jelly-ref="'+x.ref.slice(1)+'"]')?.id);
 if(JSON.stringify(fast)!==JSON.stringify(slow))throw Error('legacy parity '+q);
}
if(first('Duplicate action')!=='duplicate-enabled')throw Error('disabled order');
if(first('Viewport action')!=='onscreen-duplicate')throw Error('viewport order');
if(first('Priority action')!=='priority-enabled-offscreen')throw Error('actionability order');
if(JSON.stringify(rt.search('Save',3,0).map(get))!==JSON.stringify(['rank-exact','rank-prefix','rank-contains']))throw Error('tiers');
if(rt.resolveText('Duplicate action')?.id!=='duplicate-enabled')throw Error('resolveText');
const previous=rt.search('Save',1,0)[0].ref;
document.getElementById('rank-exact').setAttribute('aria-label','Changed');rt.markDirty();
if(!rt.search('Changed',5).some(row=>row.ref===previous))throw Error('stable ref');
"#
            }
            "shadow-targets.html" => {
                r#"
const rt=globalThis.__jellyRuntimeV1;
if(!rt.search('Shadow click',10).some(r=>r.name==='Shadow click'&&r.shadow))throw Error('open shadow');
if(!rt.search('Nested shadow click',10).some(r=>r.shadow))throw Error('nested shadow');
if(rt.search('Closed shadow action',10).length)throw Error('closed shadow leaked');
"#
            }
            _ => {
                r#"
window.__jellyBench.setCase('large');
(0,eval)(window.__baseline);
const before=globalThis.__jellyRuntimeV1;
const observe=rt=>rt.search('large',40,0).map(r=>[r.tag,r.role,r.name,r.disabled,r.in_viewport,r.shadow]);
const previous=observe(before);
(0,eval)(window.__current);
const after=globalThis.__jellyRuntimeV1;
const latest=observe(after);
if(after.version!==20||before.version!==19||after.cache.length!==1501||latest.length!==40)throw Error('runtime versions/count');
if(JSON.stringify(previous)!==JSON.stringify(latest))throw Error('v19-v20 behavior drift');
"#
            }
        };
        let script = format!(
            "<script>try{{window.__legacy={};window.__baseline={};window.__current={};{} {}document.body.setAttribute('data-e17-result','PASS')}}catch(e){{document.body.setAttribute('data-e17-result',String(e))}}</script>",
            json!(legacy),
            json!(baseline),
            json!(runtime),
            runtime,
            assertions
        );
        let html = original.replace("</body>", &format!("{script}</body>"));
        execute(&chromium, &html, &dir, fixture.trim_end_matches(".html"))?;
        println!("PASS: Chromium Rust harness {fixture}");
    }
    println!("PASS: Rust/Chromium ranking harness, 238 pure comparisons and 3 browser fixtures");
    Ok(())
}

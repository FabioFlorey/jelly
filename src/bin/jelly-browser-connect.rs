//! Operator-facing helper to register browser-level CDP endpoints without editing TOML.
use serde_json::Value as Json;
use std::{collections::HashSet, env, fs, path::PathBuf, process::Command};

// Eight alphabetic A-Z rounds: names are assigned A..Z before the next round.
const SWEETS: [&str; 208] = [
    "Applepie",
    "Biscuit",
    "Cannoli",
    "Dorayaki",
    "Eggtart",
    "Flan",
    "Gelato",
    "Halva",
    "Icecream",
    "Jalebi",
    "Kulfi",
    "Lamington",
    "Mochi",
    "Nougat",
    "Oshiruko",
    "Pavlova",
    "Qingtuan",
    "Rugelach",
    "Sakuramochi",
    "Tiramisu",
    "Uiro",
    "Vatrushka",
    "Wagashi",
    "Xingrendoufu",
    "Yokan",
    "Zabaione",
    "Alfajores",
    "Baklava",
    "Churros",
    "Dango",
    "Eclair",
    "Fudge",
    "Gulabjamun",
    "Hotteok",
    "Imarti",
    "Jellyroll",
    "Knafeh",
    "Lokum",
    "Macaron",
    "Nanaimo",
    "Oliebollen",
    "Pannacotta",
    "Qatayef",
    "Rasgulla",
    "Strudel",
    "Taiyaki",
    "Ubehalaya",
    "Vanillekipferl",
    "Watalappam",
    "Xuehuagao",
    "Youtiao",
    "Zelten",
    "Anmitsu",
    "Basbousa",
    "Castella",
    "Daifuku",
    "Ensaymada",
    "Financier",
    "Gajarhalwa",
    "Honeycake",
    "Injeolmi",
    "JianDui",
    "Kasutera",
    "Laddu",
    "Madeleine",
    "Natillas",
    "OperaCake",
    "PasteldeNata",
    "Qottab",
    "Rasmalai",
    "Sfogliatella",
    "Tresleches",
    "Unniappam",
    "Vasilopita",
    "Whoopiepie",
    "Xuemeiniang",
    "Yaksik",
    "Zefir",
    "Apfelstrudel",
    "Bebinca",
    "Cassata",
    "Dulcedeleche",
    "Etonmess",
    "Fartons",
    "Gozleme",
    "Haupia",
    "IslakKek",
    "Joconde",
    "Kheer",
    "Loukoumades",
    "Millefeuille",
    "NianGao",
    "Oatcake",
    "Picarones",
    "Quesillo",
    "Rabanada",
    "Semifreddo",
    "Tangyuan",
    "UkadicheModak",
    "VermicelliKheer",
    "Waffle",
    "Ximilu",
    "Yema",
    "Zoolbia",
    "Anpan",
    "Babka",
    "Chiacchiere",
    "Dacquoise",
    "EcclesCake",
    "FunnelCake",
    "Gingerbread",
    "Hamantaschen",
    "ItalianIce",
    "Jellybeans",
    "Krapfen",
    "Lemonbar",
    "Marzipan",
    "Namelaka",
    "OperaSlice",
    "Profiterole",
    "QueenofPuddings",
    "Ricciarelli",
    "Shortbread",
    "Tartufini",
    "UbeCake",
    "VictoriaSponge",
    "WalnutCake",
    "XiguaLao",
    "Yulelog",
    "Zimtschnecke",
    "Amaretti",
    "BaciDiDama",
    "Candies",
    "DeliziaAlLimone",
    "Ekmek",
    "Frangipane",
    "Gingersnaps",
    "HokkaidoMilkBread",
    "IcedBun",
    "JamTart",
    "KouignAmann",
    "LinzerTorte",
    "Meringue",
    "NeenishTart",
    "OrangeCake",
    "Panforte",
    "QuesadillaSalvadorena",
    "RedBeanBun",
    "Snowball",
    "Trifle",
    "UbePandesal",
    "VanillaSlice",
    "WelshCake",
    "XiangSuBing",
    "YogurtCake",
    "Zuccherini",
    "AnzacBiscuit",
    "Biscotti",
    "Crostata",
    "DulceDeLecheCake",
    "ElephantEar",
    "Florentine",
    "GingerCookie",
    "HojichaPudding",
    "IrishCreamCake",
    "JapaneseCheesecake",
    "KhanomChan",
    "LemonCurd",
    "MarronGlace",
    "Nussecke",
    "OatmealCookie",
    "PeanutBrittle",
    "Quindim",
    "RicePudding",
    "Sbrisolona",
    "TurkishDelight",
    "UpsideDownCake",
    "VienneseWhirl",
    "WalnutBrownie",
    "XiangFeiDanGao",
    "YellowCake",
    "Zserbo",
    "Amandine",
    "Bonbon",
    "CremeBrulee",
    "DanishPastry",
    "Eierkuchen",
    "FiorDiLatte",
    "Ghraybeh",
    "HazelnutPraline",
    "IndianPudding",
    "JamDoughnut",
    "KueLapis",
    "LemonMeringue",
    "Mantecados",
    "Nankhatai",
    "OrangeBlossomCake",
    "Pfeffernusse",
    "Qurabiya",
    "Rosquillas",
    "Scone",
    "Torrone",
    "UbeJam",
    "VanillaPudding",
    "Wuzetka",
    "Xiaolonggao",
    "YogurtParfait",
    "Zefiro",
];

fn registry_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("config/browsers.toml")
}

fn config_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("config/jelly.toml")
}

fn registry() -> Result<toml::Value, Box<dyn std::error::Error>> {
    let path = registry_path();
    if !path.exists() {
        return Ok(toml::Value::Table(toml::map::Map::new()));
    }
    let data = fs::read_to_string(path)?;
    Ok(data.parse::<toml::Value>()?)
}

fn entries(tree: &toml::Value) -> Vec<&toml::Value> {
    tree.get("remotes")
        .and_then(toml::Value::as_array)
        .map(|xs| xs.iter().collect())
        .unwrap_or_default()
}

fn new_uuid() -> String {
    let mut bytes: [u8; 16] = rand::random();
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}

fn next_name(tree: &toml::Value) -> String {
    let mut used: HashSet<String> = entries(tree)
        .iter()
        .filter_map(|x| x.get("name").and_then(toml::Value::as_str))
        .map(str::to_ascii_lowercase)
        .collect();
    for remote in &jelly::config::config().browser.remotes {
        if let Some(name) = &remote.name {
            used.insert(name.to_ascii_lowercase());
        }
    }
    for suffix in 1..=1000 {
        for sweet in SWEETS {
            let name = if suffix == 1 {
                format!("Jelly-{sweet}")
            } else {
                format!("Jelly-{sweet}-{suffix}")
            };
            if !used.contains(&name.to_ascii_lowercase()) {
                return name;
            }
        }
    }
    unreachable!("browser name space exhausted")
}

fn browser_endpoint(value: &str) -> Result<String, Box<dyn std::error::Error>> {
    let url = url::Url::parse(value)?;
    match url.scheme() {
        "ws" | "wss" => {
            if url.path().starts_with("/devtools/browser/") {
                Ok(value.into())
            } else {
                Err("provide a browser-level CDP WebSocket endpoint".into())
            }
        }
        "http" | "https" => {
            let mut base = url;
            base.set_path("/json/version");
            base.set_query(None);
            base.set_fragment(None);
            let output = Command::new("curl")
                .args(["--fail", "--silent", "--show-error", "--max-time", "5"])
                .arg(base.as_str())
                .output()?;
            if !output.status.success() {
                return Err("cannot discover Chromium CDP endpoint".into());
            }
            let body: Json = serde_json::from_slice(&output.stdout)?;
            let ws = body["webSocketDebuggerUrl"]
                .as_str()
                .ok_or("Chromium /json/version does not contain webSocketDebuggerUrl")?;
            // Chromium often advertises localhost from inside a container.
            // Preserve its browser target path but use the explicitly contacted host.
            let mut discovered = url::Url::parse(ws)?;
            if discovered.scheme() != "ws" && discovered.scheme() != "wss" {
                return Err("Chromium advertised an invalid WebSocket URL".into());
            }
            discovered
                .set_host(base.host_str())
                .map_err(|_| "invalid CDP host")?;
            discovered
                .set_port(base.port())
                .map_err(|_| "invalid CDP port")?;
            browser_endpoint(discovered.as_str())
        }
        _ => Err("expected ws(s) browser endpoint or http(s) Chromium debugging URL".into()),
    }
}

fn save(tree: &toml::Value) -> Result<(), Box<dyn std::error::Error>> {
    let path = registry_path();
    let temp = path.with_extension("toml.tmp");
    fs::write(&temp, toml::to_string_pretty(tree)?)?;
    fs::rename(temp, path)?;
    Ok(())
}

fn enable_remotes() -> Result<(), Box<dyn std::error::Error>> {
    let path = config_path();
    let contents = fs::read_to_string(&path)?;
    if contents.contains("backend = \"remote-cdp\"") {
        return Ok(());
    }
    if !contents.contains("backend = \"native\"") {
        return Err("set [browser].backend to remote-cdp manually".into());
    }
    // Update exactly the existing backend assignment; preserve all other settings.
    fs::write(
        path,
        contents.replacen("backend = \"native\"", "backend = \"remote-cdp\"", 1),
    )?;
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().skip(1).collect();
    let tree = registry()?;
    match args.first().map(String::as_str) {
        Some("list") if args.len() == 1 => {
            for item in entries(&tree) {
                println!(
                    "{}\t{}\t{}",
                    item.get("name").and_then(toml::Value::as_str).unwrap_or(""),
                    item.get("id").and_then(toml::Value::as_str).unwrap_or(""),
                    item.get("endpoint")
                        .and_then(toml::Value::as_str)
                        .unwrap_or("")
                );
            }
        }
        Some("add") if args.len() >= 2 => {
            let endpoint = browser_endpoint(&args[1])?;
            // Verify reachability before persisting a browser registration.
            let (mut socket, _) = tungstenite::connect(endpoint.as_str())
                .map_err(|error| format!("Chromium CDP is not reachable: {error}"))?;
            let _ = socket.close(None);
            if entries(&tree)
                .iter()
                .any(|r| r.get("endpoint").and_then(toml::Value::as_str) == Some(&endpoint))
            {
                return Err("this CDP endpoint is already registered".into());
            }
            let mut row = toml::map::Map::new();
            let id = new_uuid();
            let name = next_name(&tree);
            row.insert("id".into(), id.clone().into());
            row.insert("name".into(), name.clone().into());
            row.insert("endpoint".into(), endpoint.into());
            let mut i = 2;
            while i < args.len() {
                let key = match args[i].as_str() {
                    "--mode" => "download_mode",
                    "--connection" => "connection_mode",
                    "--remote-path" => "download_path",
                    "--host-path" => "download_host_path",
                    "--download-url" => "download_base_url",
                    _ => return Err(format!("unknown flag: {}", args[i]).into()),
                };
                i += 1;
                let value = args.get(i).ok_or("missing option value")?;
                if key == "connection_mode" && !matches!(value.as_str(), "address" | "subnet") {
                    return Err("connection must be address or subnet".into());
                }
                if key == "download_mode"
                    && !matches!(value.as_str(), "remote-only" | "shared-volume" | "transfer")
                {
                    return Err("mode must be remote-only, shared-volume or transfer".into());
                }
                row.insert(key.into(), value.clone().into());
                i += 1;
            }
            // Validate download configuration before writing.
            let mode = row
                .get("download_mode")
                .and_then(toml::Value::as_str)
                .unwrap_or("remote-only");
            let has = |key: &str| {
                row.get(key)
                    .and_then(toml::Value::as_str)
                    .is_some_and(|v| !v.is_empty())
            };
            if mode == "shared-volume" && (!has("download_path") || !has("download_host_path")) {
                return Err("shared-volume requires --remote-path and --host-path".into());
            }
            if mode == "transfer" && (!has("download_path") || !has("download_url")) {
                return Err("transfer requires --remote-path and --download-url".into());
            }
            if let Some(url) = row.get("download_base_url").and_then(toml::Value::as_str) {
                match url::Url::parse(url)?.scheme() {
                    "https" => (),
                    "http" => eprintln!(
                        "NOTICE: operator-selected HTTP artifact transfers are unencrypted; downloaded files and optional bearer tokens can be exposed in transit"
                    ),
                    _ => return Err("download URL must use HTTP or HTTPS".into()),
                }
            }
            let mut tree = tree;
            if !tree.get("remotes").is_some() {
                tree.as_table_mut()
                    .unwrap()
                    .insert("remotes".into(), toml::Value::Array(Vec::new()));
            }
            tree.get_mut("remotes")
                .and_then(toml::Value::as_array_mut)
                .ok_or("invalid registry remotes")?
                .push(toml::Value::Table(row));
            save(&tree)?;
            enable_remotes()?;
            println!(
                "Registered {name} ({id}). Restart Jelly MCP to reload browser registrations."
            );
        }
        Some("remove") if args.len() == 2 => {
            let mut tree = tree;
            let remotes = tree
                .get_mut("remotes")
                .and_then(toml::Value::as_array_mut)
                .ok_or("no browsers registered")?;
            let before = remotes.len();
            remotes.retain(|r| {
                r.get("id").and_then(toml::Value::as_str) != Some(&args[1])
                    && r.get("name").and_then(toml::Value::as_str) != Some(&args[1])
            });
            if before == remotes.len() {
                return Err("browser not found".into());
            }
            save(&tree)?;
            println!("Browser removed from registry. Restart Jelly MCP to reload.");
        }
        _ => {
            eprintln!(
                "Usage: jelly-browser-connect list | add <ws://.../devtools/browser/... or http://host:9222> [--connection address|subnet] [--mode remote-only|shared-volume|transfer] [--remote-path /downloads] [--host-path /mount] [--download-url https://...] | remove <UUID-or-name>"
            );
            std::process::exit(2);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn alphabetic_sweets_have_distinct_initials() {
        for (i, sweet) in SWEETS.iter().enumerate() {
            assert_eq!(sweet.as_bytes()[0], b'A' + (i % 26) as u8);
        }
    }
    #[test]
    fn catalog_has_at_least_one_hundred_unique_sweets() {
        assert!(SWEETS.len() >= 200);
        let unique: HashSet<_> = SWEETS
            .iter()
            .map(|name| name.to_ascii_lowercase())
            .collect();
        assert_eq!(unique.len(), SWEETS.len());
        assert_eq!(SWEETS[25], "Zabaione");
        assert_eq!(SWEETS[26], "Alfajores");
    }
    #[test]
    fn uuid_is_valid() {
        assert!(jelly::config::valid_browser_uuid(&new_uuid()));
    }
}

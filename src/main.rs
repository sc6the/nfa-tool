#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use base64::Engine;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::State;
use winreg::RegKey;

#[derive(Clone, Serialize, Deserialize)]
struct Account {
    username: String,
    token: String,
    steamid: String,
    added_at: u64,
    #[serde(default)]
    label: String,
}

#[derive(Serialize)]
struct AccountView {
    username: String,
    label: String,
    steamid: String,
    added_at: u64,
    expires_at: Option<u64>,
}

impl From<&Account> for AccountView {
    fn from(a: &Account) -> Self {
        AccountView {
            username: a.username.clone(),
            label: a.label.clone(),
            steamid: a.steamid.clone(),
            added_at: a.added_at,
            expires_at: token_claims(&a.token)
                .ok()
                .and_then(|v| v.get("exp")?.as_u64()),
        }
    }
}

#[derive(Serialize)]
struct Bootstrap {
    accounts: Vec<AccountView>,
    active_user: Option<String>,
}

struct AppData {
    accounts: Mutex<Vec<Account>>,
    load_error: Option<String>,
    steam_operation: Arc<Mutex<()>>,
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn ensure_loaded(state: &AppData) -> Result<(), String> {
    match &state.load_error {
        Some(error) => Err(error.clone()),
        None => Ok(()),
    }
}

#[tauri::command]
fn bootstrap(state: State<AppData>) -> Result<Bootstrap, String> {
    ensure_loaded(&state)?;
    let accounts = state
        .accounts
        .lock()
        .unwrap()
        .iter()
        .map(AccountView::from)
        .collect();
    Ok(Bootstrap {
        accounts,
        active_user: read_autologin_user(),
    })
}

fn update_accounts<T>(
    state: &AppData,
    update: impl FnOnce(&mut Vec<Account>) -> Result<T, String>,
) -> Result<T, String> {
    ensure_loaded(state)?;
    let mut accounts = state.accounts.lock().unwrap();
    let mut next = accounts.clone();
    let result = update(&mut next)?;
    save_accounts(&next)?;
    *accounts = next;
    Ok(result)
}

#[tauri::command]
fn add_account(line: String, state: State<AppData>) -> Result<AccountView, String> {
    let acc = build_account(&line)?;
    update_accounts(&state, |accounts| {
        if let Some(existing) = accounts.iter_mut().find(|a| a.steamid == acc.steamid) {
            existing.username = acc.username.clone();
            existing.token = acc.token.clone();
            Ok(AccountView::from(&*existing))
        } else {
            let view = AccountView::from(&acc);
            accounts.push(acc);
            Ok(view)
        }
    })
}

#[tauri::command]
fn remove_account(steamid: String, state: State<AppData>) -> Result<(), String> {
    update_accounts(&state, |accounts| {
        accounts.retain(|a| a.steamid != steamid);
        Ok(())
    })
}

#[tauri::command]
fn rename_account(steamid: String, name: String, state: State<AppData>) -> Result<(), String> {
    let name = name.trim();
    if name.chars().count() > 64 || name.chars().any(char::is_control) {
        return Err("Use a display name of up to 64 characters.".into());
    }
    update_accounts(&state, |accounts| {
        let acc = accounts
            .iter_mut()
            .find(|a| a.steamid == steamid)
            .ok_or("Account not found.")?;
        acc.label = name.to_owned();
        Ok(())
    })
}

#[tauri::command]
fn clear_all(state: State<AppData>) -> Result<(), String> {
    update_accounts(&state, |accounts| {
        accounts.clear();
        Ok(())
    })
}

#[tauri::command]
fn active_user() -> Option<String> {
    read_autologin_user()
}

#[tauri::command]
async fn login(steamid: String, state: State<'_, AppData>) -> Result<String, String> {
    ensure_loaded(&state)?;
    let acc = state
        .accounts
        .lock()
        .unwrap()
        .iter()
        .find(|a| a.steamid == steamid)
        .cloned()
        .ok_or("Account not found.")?;
    let operation = state.steam_operation.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = operation
            .try_lock()
            .map_err(|_| "Another Steam operation is in progress.")?;
        login_account(&acc)
    })
    .await
    .map_err(|_| "Steam operation failed.".to_string())?
}

#[tauri::command]
async fn logout(state: State<'_, AppData>) -> Result<String, String> {
    let operation = state.steam_operation.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = operation
            .try_lock()
            .map_err(|_| "Another Steam operation is in progress.")?;
        logout_steam()
    })
    .await
    .map_err(|_| "Steam operation failed.".to_string())?
}

fn main() {
    let (accounts, load_error) = match load_accounts() {
        Ok(accounts) => (accounts, None),
        Err(error) => (Vec::new(), Some(error)),
    };
    tauri::Builder::default()
        .manage(AppData {
            accounts: Mutex::new(accounts),
            load_error,
            steam_operation: Arc::new(Mutex::new(())),
        })
        .invoke_handler(tauri::generate_handler![
            bootstrap,
            add_account,
            remove_account,
            rename_account,
            clear_all,
            active_user,
            login,
            logout
        ])
        .run(tauri::generate_context!())
        .expect("error while running NFA Loader");
}

fn accounts_path() -> Result<PathBuf, String> {
    let base = std::env::var_os("APPDATA")
        .filter(|s| !s.is_empty())
        .ok_or("APPDATA is not available.")?;
    Ok(PathBuf::from(base).join("nfa-loader").join("accounts.dat"))
}

fn load_accounts() -> Result<Vec<Account>, String> {
    let bytes = match fs::read(accounts_path()?) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(io_msg("read saved accounts", &e)),
    };
    let plain = protect_store(&bytes, false)?;
    let accounts: Vec<Account> = serde_json::from_slice(&plain)
        .map_err(|_| "Saved accounts are damaged; the file has been left unchanged.")?;
    for acc in &accounts {
        validate_username(&acc.username)?;
        if extract_steamid_from_jwt(&acc.token)? != acc.steamid {
            return Err(
                "Saved account identity mismatch; the file has been left unchanged.".into(),
            );
        }
    }
    Ok(accounts)
}

fn save_accounts(accounts: &[Account]) -> Result<(), String> {
    let path = accounts_path()?;
    let plain = serde_json::to_vec(accounts).map_err(|_| "Could not encode accounts.")?;
    let encrypted = protect_store(&plain, true)?;
    fs::create_dir_all(path.parent().unwrap()).map_err(|e| io_msg("create account folder", &e))?;
    let temp = path.with_extension("tmp");
    {
        use std::io::Write;
        let mut file = fs::File::create(&temp).map_err(|e| io_msg("save accounts", &e))?;
        file.write_all(&encrypted)
            .and_then(|_| file.sync_all())
            .map_err(|e| io_msg("save accounts", &e))?;
    }
    fs::rename(temp, path).map_err(|e| io_msg("replace saved accounts", &e))
}

// DPAPI binds the whole store to the current Windows user. Never use machine scope here.
fn protect_store(bytes: &[u8], encrypt: bool) -> Result<Vec<u8>, String> {
    use windows::Win32::Security::Cryptography::{CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN};
    let input = CRYPT_INTEGER_BLOB {
        cbData: bytes
            .len()
            .try_into()
            .map_err(|_| "Account store is too large.")?,
        pbData: bytes.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    unsafe {
        let result = if encrypt {
            CryptProtectData(
                &input,
                windows::core::PCWSTR::null(),
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &input,
                None,
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        };
        result.map_err(|_| "Could not protect or unlock saved accounts for this Windows user. The original file has been left unchanged.")?;
        let result = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        unsafe extern "system" {
            fn LocalFree(hmem: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
        }
        LocalFree(output.pbData as *mut std::ffi::c_void);
        Ok(result)
    }
}

fn build_account(line: &str) -> Result<Account, String> {
    let (username, token) = parse_credential(line)?;
    let steamid = extract_steamid_from_jwt(&token)?;
    check_expiry(&token)?;
    Ok(Account {
        username,
        token,
        steamid,
        added_at: unix_now(),
        label: String::new(),
    })
}

fn validate_username(username: &str) -> Result<(), String> {
    if username.is_empty()
        || username.len() > 64
        || !username
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_')
    {
        return Err(
            "Use the Steam login name (letters, numbers and underscores, up to 64 characters)."
                .into(),
        );
    }
    Ok(())
}

fn parse_credential(input: &str) -> Result<(String, String), String> {
    if input.len() > 16384 {
        return Err("Account entry is too large.".into());
    }
    let (username, token) = input
        .trim()
        .split_once("----")
        .ok_or("Expected username----token.")?;
    let username = username.trim();
    let token = token.trim();
    validate_username(username)?;
    if token.is_empty() || token.chars().any(char::is_whitespace) {
        return Err("Paste one complete login token.".into());
    }
    Ok((username.to_string(), token.to_string()))
}

fn token_claims(jwt: &str) -> Result<serde_json::Value, String> {
    let parts: Vec<&str> = jwt.split('.').collect();
    if jwt.len() > 16384
        || parts.len() != 3
        || parts.iter().any(|p| {
            p.is_empty()
                || !p
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
        })
    {
        return Err("That token does not have the expected JWT format.".into());
    }
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(parts[1])
        .map_err(|_| "Could not read the token payload.")?;
    serde_json::from_slice(&payload).map_err(|_| "Could not read the token payload.".into())
}

fn extract_steamid_from_jwt(jwt: &str) -> Result<String, String> {
    let json = token_claims(jwt)?;
    let id = json
        .get("sub")
        .and_then(|v| v.as_str())
        .ok_or("Token is missing the SteamID.")?;
    let number = id.parse::<u64>().map_err(|_| "Invalid SteamID64.")?;
    if id.len() != 17
        || !id.bytes().all(|c| c.is_ascii_digit())
        || !(76561197960265729..=76561202255233023).contains(&number)
    {
        return Err("Invalid individual SteamID64.".into());
    }
    Ok(id.to_string())
}

fn check_expiry(token: &str) -> Result<(), String> {
    let claims = token_claims(token)?;
    if let Some(exp) = claims.get("exp") {
        if exp.as_u64().ok_or("Token expiry is malformed.")? <= unix_now() {
            return Err("This token has expired. Add a fresh token.".into());
        }
    }
    Ok(())
}

fn get_steam_path() -> Result<String, String> {
    let hkcu = RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
    let key = hkcu
        .open_subkey("SOFTWARE\\Valve\\Steam")
        .map_err(|_| "Steam not found. Is it installed?")?;
    let path: String = key
        .get_value("SteamPath")
        .map_err(|_| "Could not read the Steam install path.")?;
    if !Path::new(&path).is_absolute() || !Path::new(&path).join("steam.exe").is_file() {
        return Err("Steam install path is invalid.".into());
    }
    Ok(path)
}

fn local_steam_dir() -> Result<PathBuf, String> {
    let base = std::env::var_os("LOCALAPPDATA")
        .filter(|s| !s.is_empty())
        .ok_or("LOCALAPPDATA is not available.")?;
    Ok(PathBuf::from(base).join("Steam"))
}

fn check_steam_config_files(config_dir: &Path) -> Result<(), String> {
    if !config_dir.join("config.vdf").is_file() || !config_dir.join("loginusers.vdf").is_file() {
        return Err("Open Steam and sign into an account once first.".into());
    }
    Ok(())
}

// Keep the first backup; never overwrite a known pre-loader copy.
fn backup_file(path: &Path) -> Result<(), String> {
    use std::io::Write;
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(io_msg("read Steam file for backup", &e)),
    };
    let backup = path.with_extension("vdf.nfa-backup");
    let mut file = match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&backup)
    {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => return Ok(()),
        Err(e) => return Err(io_msg("create Steam backup", &e)),
    };
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| io_msg("write Steam backup", &e))
}

fn login_account(acc: &Account) -> Result<String, String> {
    validate_username(&acc.username)?;
    if extract_steamid_from_jwt(&acc.token)? != acc.steamid {
        return Err("Account identity mismatch.".into());
    }
    check_expiry(&acc.token)?;
    let steam_path = get_steam_path()?;
    let config_dir = Path::new(&steam_path).join("config");
    check_steam_config_files(&config_dir)?;
    kill_steam_process()?;
    for path in [
        config_dir.join("config.vdf"),
        config_dir.join("loginusers.vdf"),
        local_steam_dir()?.join("local.vdf"),
    ] {
        backup_file(&path)?;
    }
    // Validate the local token cache before modifying any Steam configuration.
    let local = local_steam_dir()?.join("local.vdf");
    if local.exists() {
        let content = fs::read_to_string(&local).map_err(|e| io_msg("read local.vdf", &e))?;
        inject_connect_cache(
            &content,
            &compute_crc32(&acc.username),
            &steam_encrypt(&acc.token, &acc.username)?,
        )?;
    }
    inject_account_into_config(&config_dir.join("config.vdf"), &acc.username, &acc.steamid)?;
    update_loginusers_vdf(
        &config_dir.join("loginusers.vdf"),
        &acc.username,
        &acc.steamid,
    )?;
    write_local_vdf(&acc.username, &acc.token)?;
    write_autologin_user(&acc.username)?;
    Command::new(Path::new(&steam_path).join("steam.exe"))
        .spawn()
        .map_err(|e| io_msg("start Steam", &e))?;
    Ok(format!(
        "Steam is starting with '{}'. Steam will validate the token.",
        acc.username
    ))
}

fn logout_steam() -> Result<String, String> {
    let steam_path = get_steam_path()?;
    let loginusers = Path::new(&steam_path).join("config").join("loginusers.vdf");
    kill_steam_process()?;
    backup_file(&loginusers)?;
    if loginusers.exists() {
        let content =
            fs::read_to_string(&loginusers).map_err(|e| io_msg("read loginusers.vdf", &e))?;
        fs::write(
            &loginusers,
            content.replace("\"MostRecent\"\t\t\"1\"", "\"MostRecent\"\t\t\"0\""),
        )
        .map_err(|e| io_msg("write loginusers.vdf", &e))?;
    }
    clear_autologin_user()?;
    Ok("Steam closed and automatic account selection cleared. Saved Steam sessions remain.".into())
}

fn inject_account_into_config(path: &Path, username: &str, steamid: &str) -> Result<(), String> {
    let mut content = fs::read_to_string(path).map_err(|e| io_msg("read config.vdf", &e))?;
    if content.contains(&format!("\"SteamID\"\t\t\"{}\"", steamid)) {
        return Ok(());
    }
    let block = format!(
        "\n\t\t\t\t\t\"{}\"\n\t\t\t\t\t{{\n\t\t\t\t\t\t\"SteamID\"\t\t\"{}\"\n\t\t\t\t\t}}\n",
        username, steamid
    );
    let pos = content
        .rfind("\"Accounts\"")
        .and_then(|i| content[i..].find('{').map(|o| i + o + 1))
        .ok_or("Could not find the Accounts block in config.vdf.")?;
    content.insert_str(pos, &block);
    fs::write(path, content).map_err(|e| io_msg("write config.vdf", &e))
}
fn update_loginusers_vdf(path: &Path, username: &str, steamid: &str) -> Result<(), String> {
    let mut content = fs::read_to_string(path).map_err(|e| io_msg("read loginusers.vdf", &e))?;
    content = content.replace("\"MostRecent\"\t\t\"1\"", "\"MostRecent\"\t\t\"0\"");
    content = if content.contains(&format!("\"{}\"", steamid)) {
        update_existing_user(&content, username, steamid)?
    } else {
        insert_new_user(&content, username, steamid)?
    };
    fs::write(path, content).map_err(|e| io_msg("write loginusers.vdf", &e))
}
fn current_timestamp() -> String {
    unix_now().to_string()
}
fn update_existing_user(content: &str, username: &str, steamid: &str) -> Result<String, String> {
    let mut result = String::new();
    let mut lines = content.lines();
    while let Some(line) = lines.next() {
        result.push_str(line);
        result.push('\n');
        if line.contains(&format!("\"{}\"", steamid)) {
            for inner in lines.by_ref() {
                if inner.contains("\"AccountName\"") {
                    result.push_str(&format!("\t\t\t\"AccountName\"\t\t\"{}\"\n", username));
                } else if inner.contains("\"PersonaName\"") {
                    result.push_str(&format!("\t\t\t\"PersonaName\"\t\t\"{}\"\n", username));
                } else if inner.contains("\"MostRecent\"") {
                    result.push_str("\t\t\t\"MostRecent\"\t\t\"1\"\n");
                } else if inner.contains("\"Timestamp\"") {
                    result.push_str(&format!(
                        "\t\t\t\"Timestamp\"\t\t\"{}\"\n",
                        current_timestamp()
                    ));
                } else {
                    result.push_str(inner);
                    result.push('\n');
                }
                if inner.trim() == "}" {
                    break;
                }
            }
        }
    }
    Ok(result)
}
fn insert_new_user(content: &str, username: &str, steamid: &str) -> Result<String, String> {
    let block = format!(
        r#"
	"{steamid}"
	{{
		"AccountName"		"{username}"
		"PersonaName"		"{username}"
		"RememberPassword"		"1"
		"WantsOfflineMode"		"0"
		"SkipOfflineModeWarning"		"0"
		"AllowAutoLogin"		"1"
		"MostRecent"		"1"
		"Timestamp"		"{timestamp}"
	}}
"#,
        steamid = steamid,
        username = username,
        timestamp = current_timestamp()
    );
    let pos = content.rfind('}').ok_or("loginusers.vdf is malformed.")?;
    let mut out = content.to_string();
    out.insert_str(pos, &block);
    Ok(out)
}
fn compute_crc32(data: &str) -> String {
    let v = crc32fast::hash(data.as_bytes());
    let hex = format!("{:08x}", v);
    let trimmed = hex.trim_start_matches('0');
    if trimmed.is_empty() {
        "01".to_string()
    } else {
        format!("{}1", trimmed)
    }
}
use windows::Win32::Security::Cryptography::{CryptProtectData, CRYPT_INTEGER_BLOB};
fn steam_encrypt(token: &str, account_name: &str) -> Result<String, String> {
    let data_bytes = token.as_bytes();
    let name_bytes = account_name.as_bytes();
    let data_in = CRYPT_INTEGER_BLOB {
        cbData: data_bytes.len() as u32,
        pbData: data_bytes.as_ptr() as *mut u8,
    };
    let entropy = CRYPT_INTEGER_BLOB {
        cbData: name_bytes.len() as u32,
        pbData: name_bytes.as_ptr() as *mut u8,
    };
    let desc = "BObfuscateBuffer\0";
    let desc_wide: Vec<u16> = desc.encode_utf16().collect();
    let mut data_out = CRYPT_INTEGER_BLOB::default();
    unsafe {
        CryptProtectData(
            &data_in,
            windows::core::PCWSTR(desc_wide.as_ptr()),
            Some(&entropy),
            None,
            None,
            0x11,
            &mut data_out,
        )
        .map_err(|_| "Encryption failed.".to_string())?;
        let slice = std::slice::from_raw_parts(data_out.pbData, data_out.cbData as usize);
        let hex: String = slice.iter().map(|b| format!("{:02x}", b)).collect();
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn LocalFree(hmem: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
        }
        LocalFree(data_out.pbData as *mut std::ffi::c_void);
        Ok(hex)
    }
}
fn write_local_vdf(username: &str, token: &str) -> Result<(), String> {
    let crc = compute_crc32(username);
    let encrypted = steam_encrypt(token, username)?;
    let base = local_steam_dir()?;
    let path = base.join("local.vdf");
    fs::create_dir_all(&base).map_err(|e| io_msg("create Steam folder", &e))?;
    let content = if path.exists() {
        inject_connect_cache(
            &fs::read_to_string(&path).map_err(|e| io_msg("read local.vdf", &e))?,
            &crc,
            &encrypted,
        )?
    } else {
        create_new_local_vdf(&crc, &encrypted)
    };
    fs::write(path, content).map_err(|e| io_msg("write local.vdf", &e))
}
fn inject_connect_cache(content: &str, crc: &str, encrypted: &str) -> Result<String, String> {
    let mut output = String::new();
    let lines = content.lines();
    let mut in_cc = false;
    let mut depth = 0;
    let mut replaced = false;
    for line in lines {
        let t = line.trim();
        if t == "\"ConnectCache\"" {
            in_cc = true;
            depth = 0;
            output.push_str(line);
            output.push('\n');
            continue;
        }
        if in_cc {
            if t.starts_with('{') {
                depth += 1;
            } else if t.starts_with('}') {
                depth -= 1;
                if depth == 0 && !replaced {
                    output.push_str(&format!("\t\t\t\t\t\"{}\"\t\t\"{}\"\n", crc, encrypted));
                    replaced = true;
                }
                if depth == 0 {
                    in_cc = false;
                }
            }
            if t.starts_with(&format!("\"{}\"", crc)) {
                output.push_str(&format!("\t\t\t\t\t\"{}\"\t\t\"{}\"\n", crc, encrypted));
                replaced = true;
                continue;
            }
        }
        output.push_str(line);
        output.push('\n');
    }
    if !replaced {
        return Err(
            "Existing local.vdf has no recognized ConnectCache block; left unchanged.".into(),
        );
    }
    Ok(output)
}
fn create_new_local_vdf(crc: &str, encrypted: &str) -> String {
    format!(
        r#""MachineUserConfigStore"
{{
	"Software"
	{{
		"Valve"
		{{
			"Steam"
			{{
				"ConnectCache"
				{{
					"{crc}"		"{encrypted}"
				}}
			}}
		}}
	}}
}}
"#,
        crc = crc,
        encrypted = encrypted
    )
}
use windows::Win32::System::Registry::{
    RegCloseKey, RegOpenKeyExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_SZ,
};
fn read_autologin_user() -> Option<String> {
    let hkcu = RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
    let key = hkcu.open_subkey("SOFTWARE\\Valve\\Steam").ok()?;
    let val: String = key.get_value("AutoLoginUser").ok()?;
    if val.is_empty() {
        None
    } else {
        Some(val)
    }
}
fn write_autologin_user(name: &str) -> Result<(), String> {
    unsafe {
        let subkey: Vec<u16> = "SOFTWARE\\Valve\\Steam\0".encode_utf16().collect();
        let val_name: Vec<u16> = "AutoLoginUser\0".encode_utf16().collect();
        let name_wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
        let mut hkey = HKEY::default();
        let err = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            windows::core::PCWSTR(subkey.as_ptr()),
            0,
            KEY_SET_VALUE,
            &mut hkey,
        );
        if err.0 != 0 {
            return Err(format!("Registry error: {}", err.0));
        }
        let data = std::slice::from_raw_parts(name_wide.as_ptr() as *const u8, name_wide.len() * 2);
        let err2 = RegSetValueExW(
            hkey,
            windows::core::PCWSTR(val_name.as_ptr()),
            0,
            REG_SZ,
            Some(data),
        );
        let _ = RegCloseKey(hkey);
        if err2.0 != 0 {
            return Err(format!("Registry write error: {}", err2.0));
        }
        Ok(())
    }
}
fn clear_autologin_user() -> Result<(), String> {
    write_autologin_user("")
}

fn kill_steam_process() -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    let system = std::env::var_os("SystemRoot").ok_or("SystemRoot is unavailable.")?;
    let taskkill = PathBuf::from(system).join("System32").join("taskkill.exe");
    for process in ["steam.exe", "steamwebhelper.exe"] {
        let result = Command::new(&taskkill)
            .args(["/F", "/IM", process, "/T"])
            .creation_flags(0x08000000)
            .output()
            .map_err(|e| io_msg("close Steam", &e))?;
        // taskkill returns 128 when there is no matching process.
        if !result.status.success() && result.status.code() != Some(128) {
            return Err(
                "Could not close Steam. Close Steam and any running games, then retry.".into(),
            );
        }
    }
    std::thread::sleep(Duration::from_millis(1200));
    Ok(())
}

fn io_msg(action: &str, e: &io::Error) -> String {
    if e.kind() == io::ErrorKind::PermissionDenied || e.raw_os_error() == Some(5) {
        format!(
            "Permission denied trying to {}. Check folder permissions and that Steam is closed.",
            action
        )
    } else {
        format!("Couldn't {} ({}).", action, e.kind())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn token(sub: &str, exp: u64) -> String {
        let payload = serde_json::json!({ "sub": sub, "exp": exp });
        format!(
            "e30.{}.c2ln",
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(payload.to_string())
        )
    }
    #[test]
    fn accepts_generic_credentials_and_hides_token_from_view() {
        let acc = build_account(&format!(
            "my_account----{}",
            token("76561198012345678", u64::MAX)
        ))
        .unwrap();
        let json = serde_json::to_value(AccountView::from(&acc)).unwrap();
        assert!(json.get("token").is_none());
        assert_eq!(acc.username, "my_account");
    }
    #[test]
    fn rejects_vdf_injection_and_invalid_steam_ids() {
        for name in ["bad\"name", "bad\nname", "..\\escape", "a{b}"] {
            assert!(parse_credential(&format!("{}----anything", name)).is_err());
        }
        for id in [
            "../path",
            "76561198012345678\"\n",
            "1",
            "18446744073709551615",
        ] {
            assert!(extract_steamid_from_jwt(&token(id, u64::MAX)).is_err());
        }
    }
    #[test]
    fn rejects_expired_or_malformed_tokens() {
        assert!(build_account(&format!("user----{}", token("76561198012345678", 1))).is_err());
        for jwt in ["a.b.c.d", ".e30.c2ln", "e30.%%%%.c2ln", "e30.e30."] {
            assert!(extract_steamid_from_jwt(jwt).is_err());
        }
    }
    #[test]
    fn encrypted_store_roundtrips_and_rejects_plaintext() {
        let plain = br#"[{"token":"test-secret"}]"#;
        let encrypted = protect_store(plain, true).unwrap();
        assert!(!encrypted.windows(11).any(|w| w == b"test-secret"));
        assert_eq!(protect_store(&encrypted, false).unwrap(), plain);
        assert!(protect_store(plain, false).is_err());
    }
    #[test]
    fn token_cache_preserves_other_accounts_and_unknown_content() {
        let content = create_new_local_vdf("old", "cipher1");
        let updated = inject_connect_cache(&content, "new", "cipher2").unwrap();
        assert!(updated.contains("cipher1") && updated.contains("cipher2"));
        assert!(inject_connect_cache("\"unrecognized\"\n{\n}\n", "new", "cipher").is_err());
    }
    #[test]
    fn loginusers_preserves_unrelated_fields() {
        let source = "\"users\"\n{\n\t\"76561198012345678\"\n\t{\n\t\t\"AccountName\"\t\t\"old\"\n\t\t\"MostRecent\"\t\t\"0\"\n\t\t\"OtherSetting\"\t\t\"keep\"\n\t}\n}\n";
        let updated = update_existing_user(source, "new", "76561198012345678").unwrap();
        assert!(updated.contains("\"OtherSetting\"\t\t\"keep\""));
        assert!(updated.contains("\"AccountName\"\t\t\"new\""));
    }
}

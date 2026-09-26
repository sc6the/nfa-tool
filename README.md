# NFA Loader

A minimal, OLED-black Windows loader for Steam login tokens. Works independently of any seller or shop.

Forked from [shefu223/nfa-tool](https://github.com/shefu223/nfa-tool), with a redesigned interface and security changes documented in [SECURITY-REVIEW.md](SECURITY-REVIEW.md).

## Use

1. Open Steam and sign into an account once so its configuration files exist, then close any running games.
2. Run `nfa.exe` as your normal Windows user.
3. Add `username----token`, where `username` is the **Steam login name**, not a nickname. Any source is supported if it supplies a compatible Steam login JWT. Passwords and browser cookies are not supported.
4. Click **Load** and confirm. Steam closes, its configuration is updated, and Steam restarts to validate the token.

Search accounts by name or SteamID. Rename changes only the display label. Adding the same SteamID updates its token. Expired tokens are rejected; displayed expiry information is unverified metadata, not an online account check.

Tokens are encrypted with Windows DPAPI in `%APPDATA%\nfa-loader\accounts.dat`, bound to your Windows user. The app does not send them to a shop or checker. Removing an account from the loader does not revoke its token or clear Steam's saved sessions.

The first copy of each modified Steam VDF is backed up next to the original as `.vdf.nfa-backup`. Existing `localconfig.vdf` preferences are left intact. See the security review for recovery limitations.

## Build

Requires Windows 10/11, Rust with the MSVC toolchain, Visual Studio C++ build tools, and the Microsoft Edge WebView2 runtime.

```powershell
cargo test --locked
./build.ps1
```

The build script uses the checked-in lockfile and creates `nfa.exe` in the repository root. No upstream binary is used. No administrator rights are requested automatically.

## Frontend preview and checks

```powershell
node scripts/preview.mjs
# Open http://127.0.0.1:4178/?preview=1 for read-only sample accounts.
node scripts/audit-deps.mjs
```

The audit reports all platforms and maintenance warnings, so see the review for the Windows-specific interpretation.

For browser interaction tests with a fake backend (never touches Steam):

```powershell
npx agent-browser --session nfa-tests --init-script tests/ui-bridge.js open http://127.0.0.1:4178/
Get-Content tests/ui-checks.js -Raw | npx agent-browser --session nfa-tests eval --stdin
npx agent-browser --session nfa-tests close
```

The test bridge lives outside `ui/` and is not bundled. Account switching still requires a real, authorized token and compatible Steam configuration; it has not been verified against live accounts in this fork's automated checks.

The upstream project supplied no license file; this fork retains its history and attribution and adds no license for upstream code. Not affiliated with Valve or Steam.

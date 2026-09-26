# Security review

Reviewed 2026-09-26. Upstream: `shefu223/nfa-tool`, commit `7a9c4c81b15c0c8d3036661d6998071ec737a95c`.

The upstream Rust source, frontend, build scripts, Tauri configuration/capabilities, and dependency lockfile were inspected before running application code. No upstream release executable was downloaded or executed. This is a source-level review and dependency check, not a guarantee of safety or a forensic scan of the PC.

## Findings and changes

| Finding in upstream | Impact | Fork behavior |
| --- | --- | --- |
| Automatic warranty lookup posts `username----token` to the shop; the checker also posts credentials | Third party receives account access tokens | Both services and their commands removed; no application HTTP client or updater calls |
| `%APPDATA%/shefu223-nfa/accounts.json` contains plaintext tokens | Other processes can read credentials | Separate `%APPDATA%/nfa-loader/accounts.dat`, encrypted with current-user Windows DPAPI |
| Startup automatically elevates to administrator | Broadens impact of any exploit or accidental operation | Runs with normal user permissions; no elevation or shell-open command |
| `open_url` passes arbitrary input to ShellExecute, including remote update-manifest values | Could open local executables or unsafe URI handlers | Command and update manifest removed |
| Token `sub` and account name inserted into VDF without validation | Crafted input can corrupt Steam configuration; frontend also interpolated some untrusted values as HTML | Strict individual SteamID and login-name validation; frontend uses textContent |
| “Clear Steam” deletes configuration files and recursively removes local Steam data | Destructive loss of Steam state | Entire feature and backend command removed |
| Login replaces `localconfig.vdf` and may replace unrecognized `local.vdf` content | Existing preferences/token cache can be lost | Never writes localconfig.vdf; rejects an unrecognized token cache; first-use backups before supported changes |
| Saving ignores errors; malformed account store becomes an empty list | Silent loss of saved accounts | Save errors propagate; memory changes only after successful save; unreadable stores block mutations |
| Renaming changes the Steam login name | Subsequent login/token encryption can fail | Independent display label; real login name preserved |
| Overlapping Steam operations | Configuration races | Backend serializes Steam operations; UI disables mutations while busy |

## Dependencies

OSV's batch endpoint was queried using only public crate names/versions. The upstream lockfile contained 446 registry packages; after removing the direct HTTP client and updating `plist`, the fork contains 425.

- `quick-xml` 0.39.4 had [RUSTSEC-2026-0194](https://rustsec.org/advisories/RUSTSEC-2026-0194.html) and [RUSTSEC-2026-0195](https://rustsec.org/advisories/RUSTSEC-2026-0195.html), XML denial-of-service issues. Updated its parent `plist` to 1.10.1, resolving `quick-xml` to 0.42.0. Both findings disappeared from the follow-up query.
- `glib` 0.18.5 retains [RUSTSEC-2024-0429](https://rustsec.org/advisories/RUSTSEC-2024-0429.html) in the cross-platform lockfile. It is absent from the Windows dependency graph (`cargo tree --target x86_64-pc-windows-msvc -i glib`). This app is Windows-only.
- `proc-macro-error` 1.0.4 is unmaintained; also absent from the Windows graph.
- The five `unic-*` crates used transitively by Tauri's `urlpattern` are [unmaintained](https://rustsec.org/advisories/RUSTSEC-2025-0100.html). These maintenance warnings remain; no patched versions are offered. They are not suppressed by the audit script.

Run `node scripts/audit-deps.mjs` to repeat the advisory lookup. It returns a nonzero exit code for any finding, including maintenance notices and non-Windows dependencies. Results depend on the advisory database at query time.

## Boundaries and residual risk

- Tokens are credentials. Use accounts you own or have permission to access. DPAPI does not protect against malware already running as your Windows user, administrator-level compromise, or memory inspection.
- JWT decoding checks syntax, SteamID and an expiry claim when present. It does **not** verify the signature, revocation, ownership, or successful Steam authentication. The interface labels token metadata unverified.
- Loading an account deliberately closes Steam and its child processes, updates `config.vdf`, `loginusers.vdf`, `%LOCALAPPDATA%/Steam/local.vdf`, and the current user's `AutoLoginUser` registry value, then starts installed Steam. Steam itself uses the network. Close games first.
- VDF editing inherits line-oriented assumptions from upstream. Steam can change these formats. Multi-file changes are not a transaction; an I/O failure can leave partial changes. Backups preserve the first pre-loader copy beside each affected VDF as `.vdf.nfa-backup`; inspect these before restoring manually with Steam closed.
- Remove/remove-all affects this loader's encrypted store only. Close Steam / sign out clears automatic selection, not Steam's saved credentials or token validity.
- Old upstream plaintext stores are not imported or deleted automatically. If you used upstream previously, its file remains separate and may still contain tokens.
- No telemetry, shop checker, remote warranties, external links, automatic updates, arbitrary process-launch command, or Steam-data wipe is included. Bundled Tauri/WebView dependencies still require ongoing maintenance.
- End-to-end Steam authentication was not exercised with real credentials. Tests use synthetic tokens, temporary data, or a browser-only bridge outside the packaged frontend.

## Verification

- Rust tests cover credential validation, injection-shaped inputs, expired/malformed tokens, encrypted store round trips, token omission from frontend account views, and preservation of unrelated VDF entries.
- Browser checks cover add/update, search, renaming without changing login identity, HTML-injection-shaped display names, cancellation, selected-account rendering, save failure/retry, and empty states.
- Release build, `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings`, six Rust tests, and twelve browser interaction checks passed.
- Packaged WebView2 app smoke test passed with an isolated APPDATA directory: native IPC, account save, display rename, token omission from views, rejection of injection-shaped input, encrypted store bytes, persistence across a full app restart, and removal of the test account. No real Steam login/logout was invoked.
- Windows Defender custom-scanned the final executable and reported **no threats** (exit code 0). Real-time protection was enabled. Antivirus detection is not proof that software is harmless.
- Scanned executable SHA-256: `2789FD4AD02ED00DD3D5C42C4DE2DBF23C76DCC5B2C41274BDEF7E0E42018A3E`.

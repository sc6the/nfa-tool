// Browser-test fixture only. This file is outside the packaged ui/ directory.
(() => {
  const accounts = [];
  let active = null;
  window.testCalls = [];
  window.__TAURI__ = {
    window: { getCurrentWindow: () => ({ minimize() {}, close() {} }) },
    core: { async invoke(command, args = {}) {
      window.testCalls.push(command);
      if (window.failNextCommand === command) { window.failNextCommand = null; throw 'Simulated disk failure'; }
      switch (command) {
        case 'bootstrap': return { accounts: structuredClone(accounts), active_user: active };
        case 'active_user': return active;
        case 'add_account': {
          const [username, token] = args.line.split('----');
          if (!username || !token) throw 'Expected username----token.';
          const a = { username, label: '', steamid: '76561198012345678', added_at: 1, expires_at: 2000000000 };
          const index = accounts.findIndex(x => x.steamid === a.steamid);
          if (index < 0) accounts.push(a); else accounts[index] = a;
          return structuredClone(a);
        }
        case 'rename_account': accounts.find(x => x.steamid === args.steamid).label = args.name; return;
        case 'remove_account': accounts.splice(accounts.findIndex(x => x.steamid === args.steamid), 1); return;
        case 'clear_all': accounts.length = 0; return;
        case 'login': active = accounts.find(x => x.steamid === args.steamid).username; return 'Simulated Steam launch.';
        case 'logout': active = null; return 'Simulated Steam closure.';
        default: throw `Unexpected command: ${command}`;
      }
    } },
  };
})();

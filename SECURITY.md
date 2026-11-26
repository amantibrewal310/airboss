# Security

Airboss runs on your Mac, opens links that agents can add, and reads other tools' files, so a
security bug matters. Please report one privately: on GitHub, **Security › Report a
vulnerability** in this repo. Don't open a public issue for it.

Say what an attacker could do, and how. You'll get a reply within a few days.

Only the latest release is supported.

## Updates

From 0.1.0-alpha.4, Airboss updates itself. Each update archive is signed with an updater key
(minisign, through Tauri's updater) whose public half is built into the app, and the app
refuses a download whose signature doesn't match, whatever the manifest says. The private key
never leaves the maintainer's Mac and isn't in this repository. The manifest is
`updates/latest.json` on `main`, and the archives are this repo's GitHub release assets.

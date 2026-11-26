# Updates

`latest.json` here is the manifest installed copies of Airboss read to find a new version
([SPEC-updates](../SPEC-updates.md)). `app/src-tauri/scripts/update-manifest.mjs` writes it after
each release is published, and that commit on `main` is what makes the release reach installed
apps. Each update archive it names is signed; the app checks the signature before installing.

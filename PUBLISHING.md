# Packaging and publishing

The applet is published in the COSMIC Store through
[`pop-os/cosmic-flatpak`](https://github.com/pop-os/cosmic-flatpak) as
`io.github.Zetakai.cosmic-ext-applet-crypto`.

## Why not Flathub

[Flathub's requirements](https://docs.flathub.org/docs/for-app-authors/requirements)
exclude this whole class of software on two counts:

> Shell, window manager, desktop environment extensions will not be accepted.

> Applications that operate exclusively as tray applets will not be accepted.

A COSMIC panel applet is both. Do not spend time on a Flathub submission.
`cosmic-flatpak` exists precisely for "applets and other flatpaks for COSMIC that are
not suitable for upload to Flathub", and its remote ships configured on COSMIC
systems, so apps merged there appear in the COSMIC Store.

## Shipping an update

1. **Make the change and bump the version.**

   ```bash
   # Cargo.toml: version = "0.1.2"
   # resources/app.metainfo.xml: add a <release> entry at the top of <releases>
   cargo test
   ```

   The `<release>` entry matters — the store shows it as the changelog.

2. **Tag and release.**

   ```bash
   git tag -a v0.1.2 -m "Short summary"
   git push origin main && git push origin v0.1.2
   gh release create v0.1.2 --title "v0.1.2" --notes "..."
   ```

3. **Regenerate the offline sources, but only if dependencies changed.**

   `cargo-sources.json` is the dependency set, not the package. A version bump alone
   does not change it; a `Cargo.lock` change from adding or updating a dependency
   does.

   ```bash
   python3 -m venv /tmp/fcg && /tmp/fcg/bin/pip install aiohttp toml tomlkit
   curl -fsSLO https://raw.githubusercontent.com/flatpak/flatpak-builder-tools/master/cargo/flatpak-cargo-generator.py
   /tmp/fcg/bin/python flatpak-cargo-generator.py Cargo.lock -o flatpak/cargo-sources.json
   ```

4. **Point the manifest at the new tag.**

   Update `tag` and `commit` in
   `flatpak/io.github.Zetakai.cosmic-ext-applet-crypto.json`. The commit must be the
   one the tag points at: `git rev-parse v0.1.2^{commit}`.

5. **Open the PR against `cosmic-flatpak`.**

   ```bash
   git clone https://github.com/<you>/cosmic-flatpak.git && cd cosmic-flatpak
   git remote add upstream https://github.com/pop-os/cosmic-flatpak.git
   git fetch upstream master
   git checkout -b update-crypto-0.1.2 upstream/master
   cp ~/Documents/GitHub/cosmic-ext-applet-crypto/flatpak/* \
      app/io.github.Zetakai.cosmic-ext-applet-crypto/
   git commit -am "Update io.github.Zetakai.cosmic-ext-applet-crypto to v0.1.2"
   ```

   Open it against `master` — the `new-pr` branch rule is Flathub's, not theirs.

   **Fill in their PR template.** GitHub pre-fills it from
   `.github/PULL_REQUEST_TEMPLATE.md`; do not replace it with your own description,
   which is what got the first submission sent back. It requires disclosing AI
   generated code **in the commit messages**, not only in the PR body.

6. **CI builds it.** A first-time contributor's run needs a maintainer to approve it.

## Checking before you submit

```bash
flatpak run --command=flatpak-builder-lint org.flatpak.Builder manifest \
  app/io.github.Zetakai.cosmic-ext-applet-crypto/io.github.Zetakai.cosmic-ext-applet-crypto.json
```

No output means it passed.

It currently reports one known error, `appid-url-not-reachable`: the App ID encodes
`Zetakai` while the repository now lives under `Gliana-Labs`, so the derived URL
301-redirects and the linter treats that as unverified. The build is unaffected and
`cosmic-flatpak` CI runs the build rather than the linter. See below.

A full local build needs the SDK extension, which is a large download:

```bash
flatpak install --user flathub org.flatpak.Builder \
  com.system76.Cosmic.BaseApp//stable org.freedesktop.Sdk.Extension.rust-stable//25.08
just build io.github.Zetakai.cosmic-ext-applet-crypto
```

Note `runtime-version` must stay at `25.08`. `24.08` ships Rust 1.89 and the build
fails on `libcosmic 1.0.0 requires rustc 1.93`.

## Deferred: renaming the App ID

Not urgent, and deliberately not done yet. Recorded so the reasoning is not lost.

The App ID is `io.github.Zetakai.cosmic-ext-applet-crypto`, from when the repository
was under an individual account. The project now belongs to Gliana Labs, so the ID no
longer reflects its owner.

`io.github.Gliana-Labs.…` is **not** a valid alternative: App IDs allow a dash only
in the final component, and `Gliana-Labs` has one in the middle. The correct form,
since Gliana Labs owns `glianalabs.com`, is:

```
com.glianalabs.CosmicExtAppletCrypto
```

**Why it has not been done:** an App ID change creates a *new* flatpak app. The old
one has to be marked end-of-life and rebased onto the new one, or existing installs
are stranded. That is churn to ask of a maintainer for an app that had only just been
merged, in exchange for tidiness — the ID is an opaque identifier, and users see
"Crypto Prices" by "Gliana Labs" either way.

**What it would take, when it is worth doing:**

1. Change `APP_ID` in `src/app.rs`, the manifest `app-id`, the icon filenames, the
   metainfo `<id>` and `<launchable>`, and the desktop file name.
2. Migrate settings: the config path
   `~/.config/cosmic/<app-id>/` is derived from the ID, so existing users lose their
   coin list unless the applet copies the old directory forward on first run.
3. Tag a release and open a PR adding `app/com.glianalabs.CosmicExtAppletCrypto/`.
4. Add a line to `cosmic-flatpak`'s `end-of-life.txt` in the same PR:

   ```
   io.github.Zetakai.cosmic-ext-applet-crypto=com.glianalabs.CosmicExtAppletCrypto
   ```

   Their `just eol` recipe reads that file and publishes an end-of-life rebase, which
   migrates installed copies automatically.
5. Keep the old directory until the rebase has propagated.

Doing this also clears the `appid-url-not-reachable` lint error, since
`https://glianalabs.com` resolves directly.

## Store listing gotchas

- **Developer name.** cosmic-store reads `developer_name` and nothing else.
  `src/app_info.rs` takes `component.developer_name`, and `src/view.rs` falls back to
  the `app-developers` string — `{$app} Developers` — when it is empty, which is why
  this listing first showed "Crypto Prices Developers". Its appstream dependency is a
  fork ([jackpot51/appstream](https://github.com/jackpot51/appstream)), so the newer
  `<developer><name>` is not read there; do not drop `developer_name` on the
  assumption that the modern tag has started working. The metainfo carries both,
  since upstream AppStream deprecated `developer_name` in 1.0.
- **Screenshots** come from the URLs in the metainfo, which point at `raw.githubusercontent.com`
  on `main`. They are fetched when the repository is rebuilt, so a screenshot must be
  committed and the tag must contain it.
- **Trademark.** [COSMIC's policy](https://github.com/pop-os/cosmic-epoch/blob/master/TRADEMARK.md)
  reserves the `cosmic-` package namespace and `com.system76.` App ID prefix for
  official software, and directs third-party applets to `cosmic-ext-`. An applet may
  be described as "for the COSMIC™ desktop" but should not lead with the trademark in
  its name.

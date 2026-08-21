# Publishing to the COSMIC Store

## Flathub will reject this app

Not a matter of quality — [Flathub's requirements](https://docs.flathub.org/docs/for-app-authors/requirements)
exclude this whole class of software, on two counts:

> Shell, window manager, desktop environment extensions will not be accepted.

> Applications that operate exclusively as tray applets will not be accepted.

A COSMIC panel applet is both. Do not spend time on a Flathub submission.

## The actual route: pop-os/cosmic-flatpak

[`pop-os/cosmic-flatpak`](https://github.com/pop-os/cosmic-flatpak) exists precisely
for "applets and other flatpaks for COSMIC that are not suitable for upload to
Flathub". It already hosts several dozen COSMIC applets.

Its remote ships configured on COSMIC systems, so apps merged there appear in the
COSMIC Store:

```
$ flatpak remotes
cosmic    https://apt.pop-os.org/cosmic/    user
```

## Already done

- `flatpak/io.github.zetakai.CosmicAppletCrypto.json` — the manifest, pinned to the
  `v0.1.0` tag and its commit
- `flatpak/cargo-sources.json` — 1406 offline dependency entries, required because
  the build runs with `--offline`
- `resources/app.metainfo.xml` — description, developer, URLs, releases, screenshot
  entry, all of which the store displays
- `v0.1.0` tagged, pushed, and released on GitHub

The manifest carries `--share=network`, which the applets it was modelled on do not
need and this one cannot work without.

## Step 1 — take a screenshot

The metainfo points at `resources/screenshots/popup.png`, which does not exist yet.
The store shows this, and a submission without one is worth much less.

Open the popup with a few coins tracked, then:

```bash
# COSMIC's screenshot tool, or any tool you prefer
cosmic-screenshot
mv <captured file> resources/screenshots/popup.png
git add resources/screenshots/popup.png
git commit -m "Add popup screenshot"
git push
```

Then move the `v0.1.0` tag so the release contains it:

```bash
git tag -f -a v0.1.0 -m "First release"
git push -f origin v0.1.0
```

and update `commit` in the manifest to the new `git rev-parse v0.1.0^{commit}`.

## Step 2 — build it locally first

A submission that fails CI wastes a reviewer's time.

```bash
sudo apt-get install flatpak flatpak-builder just
flatpak remote-add --if-not-exists --user flathub https://dl.flathub.org/repo/flathub.flatpakrepo

git clone https://github.com/pop-os/cosmic-flatpak.git
cd cosmic-flatpak
mkdir -p app/io.github.zetakai.CosmicAppletCrypto
cp ~/Documents/GitHub/cosmic-applet-crypto/flatpak/* app/io.github.zetakai.CosmicAppletCrypto/

just build io.github.zetakai.CosmicAppletCrypto
```

The build pulls a large dependency tree and takes a while. Then install and confirm
it actually runs as a panel applet, which is the part no amount of manifest review
can tell you:

```bash
flatpak install --user repo io.github.zetakai.CosmicAppletCrypto
flatpak run io.github.zetakai.CosmicAppletCrypto
```

## Step 3 — open the pull request

```bash
# on your fork of pop-os/cosmic-flatpak
git checkout -b add-cosmic-applet-crypto
git add app/io.github.zetakai.CosmicAppletCrypto
git commit -m "Add io.github.zetakai.CosmicAppletCrypto"
git push origin add-cosmic-applet-crypto
```

Open the PR against `master`. CI runs `just build-changed`, which builds only the
manifests your PR touches.

Note this differs from Flathub, which requires PRs against a `new-pr` branch. That
rule does not apply here.

## Step 4 — after merge

The repository is rebuilt and signed by the maintainers, and the app appears in the
COSMIC Store for anyone with the `cosmic` remote.

To ship an update: tag a new release, regenerate `cargo-sources.json` if
dependencies changed, and open a PR bumping `tag` and `commit` in the manifest.

## Regenerating cargo-sources.json

Needed whenever `Cargo.lock` changes.

```bash
python3 -m venv /tmp/fcg && /tmp/fcg/bin/pip install aiohttp toml tomlkit
curl -fsSLO https://raw.githubusercontent.com/flatpak/flatpak-builder-tools/master/cargo/flatpak-cargo-generator.py
/tmp/fcg/bin/python flatpak-cargo-generator.py Cargo.lock -o flatpak/cargo-sources.json
```

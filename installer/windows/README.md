# このフォルダについて / About this folder

`open-redmine.iss` はこのインストーラーを**作るための** [Inno Setup](https://jrsoftware.org/isinfo.php)
ビルドスクリプトです。`open-redmine-installer.exe` はこのフォルダ内に実体として置いてあります
(ユーザー指示により、ローカルビルドしたバイナリを直接コミット)。

**⬇ 今すぐダウンロード**: [open-redmine-installer.exe](open-redmine-installer.exe)

**正直な開示**: このファイルはビルド成果物であり、`open-redmine.iss`やソースコードを変更しても
自動的には更新されません(手動での再ビルド・再コミットが必要)。現時点でこのインストーラーを
自動ビルド・公開するCIは無いため、このファイルが唯一の配布経路です(`.github/workflows/release.yml`は
`rs-chiketto`バイナリのtarball/zip配布のみを行い、この`.exe`インストーラー自体はビルドしません)。

---

# About this folder

`open-redmine.iss` is the [Inno Setup](https://jrsoftware.org/isinfo.php) build script used to
**produce** this installer. `open-redmine-installer.exe` itself is committed directly into this
folder (per explicit user instruction, a locally-built binary is committed rather than only
published elsewhere).

**⬇ Download now**: [open-redmine-installer.exe](open-redmine-installer.exe)

**Honest disclosure**: this file is a build artifact. Changes to `open-redmine.iss` or the source
code do not automatically update it (a manual rebuild and re-commit is required). There is
currently no CI that automatically builds and publishes this installer, so this file is the only
distribution path (`.github/workflows/release.yml` only publishes the `rs-chiketto` binary
tarball/zip, not this `.exe` installer).

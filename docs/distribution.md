# Shipping omacal: testers, production, open source

What it takes to put omacal in other people's hands — a handful of testers
first, the public eventually — recorded while the reasoning is fresh
(2026-08-11). Nothing here is built yet unless it says so; this is the map,
not the territory.

The one fact everything below hangs off: **the OAuth client credentials are
the real barrier, not packaging.** Today every user hand-creates a Google
Cloud project and writes `~/.config/omacal/config.toml`. No normal person
will do that, and no step below makes sense until that stops being required.

## 1. Embed the OAuth client in the build

The `client_id`/`client_secret` pair identifies **the application**, not any
person. When a user signs in, omacal presents its client id, Google shows the
consent screen, the user approves on their own account, and the refresh token
— theirs, minted for them — lands in their keychain. Embedding the pair just
lets that dance start without a per-user Cloud project.

**The secret is not secret, by Google's own position.** For the installed-app
flow their docs state the client secret is not treated as confidential —
anything shipped in a binary is extractable by definition. Sign-in security
rests on the consent screen, the loopback redirect (the token returns only to
a process on the user's machine), and PKCE. Thunderbird, GNOME Online
Accounts and rclone all ship their pairs in public source. The realistic
worst case of extraction is brand impersonation — someone's consent screen
says "omacal" — which still cannot touch anyone's data without that person
clicking Approve, and the secret can be rotated in the Cloud Console at any
time (a new build picks it up).

**Design (agreed, not yet implemented):**

- Compile-time `option_env!("OMACAL_CLIENT_ID")` / `OMACAL_CLIENT_SECRET`,
  baked only when the release build sets them:

      OMACAL_CLIENT_ID=… OMACAL_CLIENT_SECRET=… cargo tauri build

- **Precedence: `config.toml` wins when present**; the embedded pair is the
  fallback; only when neither exists does today's "no config at …" error
  appear. Developers and distro packagers keep using their own projects, and
  dev builds on this machine (no env vars set) behave exactly as today.
- The pair is **never committed**. In CI it arrives as a GitHub Actions
  secret at release-build time. It is extractable from binaries regardless —
  keeping it out of source dodges scrapers and keeps rotation meaningful.

## 2. The Google consent screen, per audience

All users of the official binaries share one Cloud project, so its state is
the product's state:

| Consent screen state | What users experience |
| --- | --- |
| **Testing** | Only listed test users (max 100) may sign in, and their refresh tokens **expire every 7 days** — the trap `running-on-macos.md` already documents. Never ship testers this. |
| **Production, unverified** | Anyone may sign in through a "Google hasn't verified this app" interstitial (Advanced → continue), but a **100-user cap** applies to new grants for sensitive scopes. Fine for a handful of testers; a wall for open source. |
| **Production, verified** | No warning, no cap. Required for real distribution. |

**Verification** is the long pole — start it early. Calendar scopes are
*sensitive* but not *restricted*, so it is the free review (homepage, privacy
policy, demonstrating scope use — GitHub Pages satisfies the first two), not
the paid security assessment Gmail/Drive apps need. Days to weeks, once.
Before submitting, **minimize scopes**: request `calendar.events` plus a
read-only calendar list rather than the full `calendar` scope if that covers
what sync and the write paths actually do — narrower requests review faster
and read less scary on the consent screen.

**Quota** is shared across all users of the embedded client. Calendar's
default (~1M queries/day) supports thousands of installs syncing every 5
minutes; the `config.toml` escape hatch is also the documented courtesy exit
(rclone's own pattern) for anyone who wants their own quota.

## 3. Artifacts, per platform

### Linux

- `cargo tauri build` produces `.deb`, `.rpm` and an **AppImage** (the
  bundler tooling downloads on first use). The AppImage is the
  hand-someone-one-file answer: download, `chmod +x`, run — Arch, Ubuntu,
  Fedora alike. `--no-bundle` (what this machine uses day-to-day) skips all
  of this and emits only the bare binary.
- **AUR package** for Arch/Omarchy users — the native path on this app's home
  platform. Eventually **Flathub** for the widest reach; package managers
  also solve updates.
- The one honest caveat for the README: sign-in stores the refresh token in
  the Secret Service, so a keyring daemon (gnome-keyring / KeePassXC) must be
  running. Desktop GNOME/KDE users have one by default; minimal-WM setups are
  the exception, and `running-on-omarchy.md` documents the fix.

### macOS

- Must be **built on a Mac** — no realistic cross-compile for Tauri. One
  command produces the `.dmg`; that session is also the moment to regenerate
  the stale darwin snapshot baselines (`npx playwright test
  components.spec.ts --update-snapshots`).
- **Unsigned**: Gatekeeper blocks on first open; right-click → Open works and
  is acceptable for a handful of testers, and nobody else.
- **Real distribution needs an Apple Developer ID** ($99/yr): Developer ID
  signing plus notarization removes the friction entirely. Distribute via
  GitHub Releases and a **Homebrew cask**.

## 4. Handover to a few testers (the short version)

1. Implement §1 (embedded credentials); confirm the consent screen is in
   Production.
2. Build: AppImage here, `.dmg` on the Mac (unsigned is fine at this scale).
3. A GitHub Release with both artifacts and a `TESTING.md`: install per OS,
   the one-time unverified-app click-through, the Gatekeeper right-click, the
   Linux keyring caveat, where to report.
4. If the repo is private: grant read access, or just send the files.

## 5. Open source, the additional furniture

- **LICENSE** — MIT, or the Rust-conventional MIT/Apache-2.0 dual.
- **README** with a normal-person quickstart (install → launch → connect);
  the existing `docs/` already carry the deep guides.
- **CI**: build + both suites on a Linux/macOS matrix; a tag-triggered
  release workflow that builds all artifacts with the credentials injected
  from repository secrets.
- Issue templates, and the scope-minimization audit from §2 done before the
  verification submission rather than after.

## 6. Order of work

1. Credentials embedding (§1) — unblocks everything else.
2. LICENSE + README + CI.
3. Verification submission (§2) — the long pole, so early.
4. Packaging polish: AUR first, then Homebrew + notarization, Flathub last.

Only two pieces cannot be built from this chair: the Google verification
submission and the Apple Developer ID both belong to the project owner.

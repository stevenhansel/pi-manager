# pim

Profile manager and launcher for [pi](https://pi.dev). Run independent
profiles — each with their own extensions, skills, MCP servers, auth,
sessions, and config files — **simultaneously** in different terminals.

## How it works

Pi supports the `PI_CODING_AGENT_DIR` environment variable (see `pi --help`),
which overrides the default `~/.pi/agent` config directory. `pim` stores
profiles as directories under `~/.pim/profiles/<name>/`. Inside each profile directory, there is a `manifest.json` file.
When you run `pim <profile>`, it builds/refreshes the profile's directory and **exec's into `pi`** with
`PI_CODING_AGENT_DIR` set directly to `~/.pim/profiles/<name>/`.

This means:
- **Multiple profiles can run at the same time** in different terminals
- **No global state** — each pi instance has its own sessions, auth, configs
- **Never touches `~/.pi/agent`** — your default pi config is left alone
- **No more crashes** — switching profiles never affects running pi processes

### First-time migration

If you already have a real `~/.pi/agent/` directory from using pi normally,
the first time you run `pim set-default <name>`, it will automatically migrate
your existing config into the new resource pool format.

## Installation

Requires [pi](https://pi.dev) (`npm install -g pi-coding-agent`). Choose one of the installation methods below:

### 1. APT Repository (Debian / Ubuntu / Raspberry Pi OS)
Download and trust the repository public key:
```bash
sudo mkdir -p /etc/apt/keyrings
curl -fsSL https://stevenhansel.github.io/pi-manager/public.gpg | sudo gpg --dearmor -o /etc/apt/keyrings/pi-manager.gpg
```
Add the repository source:
```bash
echo "deb [signed-by=/etc/apt/keyrings/pi-manager.gpg] https://stevenhansel.github.io/pi-manager stable main" | sudo tee /etc/apt/sources.list.d/pi-manager.list
```
Update and install:
```bash
sudo apt-get update && sudo apt-get install pim
```

### 2. Nix Flake
To run `pim` instantly without installing:
```bash
nix run github:stevenhansel/pi-manager -- --help
```
To install it in your user profile:
```bash
nix profile install github:stevenhansel/pi-manager
```

### 3. Pre-compiled Binaries (GitHub Releases)
You can download pre-compiled release binaries for Linux, macOS, and Windows directly from the [Releases Page](https://github.com/stevenhansel/pi-manager/releases). Extract the archive and copy the `pim` binary to any directory on your `$PATH`.

### 4. Build from Source (Cargo)
If you have [Rust](https://rustup.rs/) installed, you can build and install the binary directly from source:
```bash
cargo install --git https://github.com/stevenhansel/pi-manager.git
```

## Usage

`pim` is both a **profile manager** and a **pi launcher**. When you give it a
profile name, it builds that profile's config and exec's into `pi` with
`PI_CODING_AGENT_DIR` set — no global state, no symlink tricks.

### Launch pi with a profile

```bash
# Launch pi with your default profile
pim

# Launch pi with a specific profile (one-shot, doesn't change default)
pim research

# Launch pi with profile and pass args through
pim work -- -p "fix the bug"
pim research -p "what's the weather?"
```

### Manage profiles

```bash
# Create a new empty profile
pim create work

# Create from your current ~/.pi/agent config
pim create work --from-base

# Copy selections from an existing profile
pim create experiments --from work

# Edit profile selections (extensions, skills, prompts) interactively
pim edit work

# List all profiles
pim list

# Build/refresh the profile directory and set it as the default
pim set-default work

# Show current status
pim status

# Delete a profile
pim delete experiments
pim delete experiments --force   # skip confirmation

```

### Quick reference

| Command | What it does |
|---------|-------------|
| `pim` | Launch pi with default profile |
| `pim research` | Launch pi with that profile |
| `pim -p "hello"` | Launch pi with default profile + args |
| `pim work -- -p "hi"` | Launch pi with profile + args |
| `pim set-default work` | Build/refresh profile + set as default |
| `pim edit work` | Edit profile selections interactively |
| `pim list` | List all profiles |
| `pim create work` | Create a new profile |
| `pim delete work` | Delete a profile |
| `pim status` | Show current status |

## What a profile looks like

A profile is a directory under `~/.pim/profiles/<name>/` containing a `manifest.json` file:

```json
{
  "select": {
    "extensions": ["rtk.ts"],
    "skills": ["web-research"]
  },
  "settings": {
    "theme": "dark"
  },
  "configs": {
    "searxng.json": {
      "baseUrl": "https://searxng.example.com"
    }
  }
}
```

When built or used, the profile directory itself becomes the pi coding agent folder:

```
~/.pim/profiles/work/
├── manifest.json     ── configuration manifest
├── settings.json     ── generated from manifest
├── mcp.json          ── generated from manifest
├── config/           ── seeded defaults + manifest config overrides
├── extensions/       ── symlinks → pool/extensions/
├── skills/           ── symlinks → pool/skills/
├── prompts/          ── symlinks → pool/prompts/
├── auth.json         ── runtime auth file
├── models.json       ── runtime models file
├── trust.json        ── runtime trust file
└── sessions/         ── runtime sessions directory
```

Key differences from the old design:
- **`~/.pi/agent` is never touched** — your default pi config is preserved
- **Profile directory IS the agent directory** — no separate `.active/` or `data/` directories, keeping all state fully consolidated inside `profiles/<name>/`
- **Isolated running** — Multiple profiles can run simultaneously in different terminals without any risk of interference

### How `PI_CODING_AGENT_DIR` works

Pi reads `PI_CODING_AGENT_DIR` on startup to locate its config directory.
pim sets this to the profile's directory before launching pi:

```bash
# What pim does internally:
PI_CODING_AGENT_DIR=~/.pim/profiles/research exec pi
```

You can also use this directly if you want to run pi with a specific
profile without pim:

```bash
PI_CODING_AGENT_DIR=~/.pim/profiles/research pi -p "hello"
```

### What happens to `~/.pi/agent`?

`~/.pi/agent` is **never touched by pim**. If you had a pre-existing
`~/.pi/agent` from using pi directly, it remains intact as your default
pi config. Running `pi` directly (without pim) will continue to use it.

## Development

### Pre-commit hooks

This repo ships with git hooks in `.githooks/` that run the same lint checks as CI
(`cargo fmt --check` and `cargo clippy --all-targets --all-features -- -D warnings`)
before every commit. Enable them with:

```bash
git config core.hooksPath .githooks
```

Now `git commit` will block if formatting or clippy issues are detected.

### Release workflow

Pushing a tag matching `v*` triggers the [release workflow](.github/workflows/release.yml)
which builds binaries for Linux, macOS, and Windows, publishes a GitHub Release,
and updates the APT repository on the `gh-pages` branch.

## Architecture

pim uses a **resource pool configuration model** — see [`docs/configuration.md`](docs/configuration.md) for the full design.

In short:

- **`pool/`** — global source of truth for extensions, skills, and prompts
- **`profiles/<name>/`** — profile-specific directory containing `manifest.json`, symlinked resources, and runtime state (auth, config, sessions)
- **`~/.pi/agent`** — **never touched by pim** — left as your default pi config

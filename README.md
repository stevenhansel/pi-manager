# pim

Profile manager and launcher for [pi](https://pi.dev). Run independent
profiles — each with their own extensions, skills, MCP servers, auth,
sessions, and config files — **simultaneously** in different terminals.

## How it works

Pi supports the `PI_CODING_AGENT_DIR` environment variable (see `pi --help`),
which overrides the default `~/.pi/agent` config directory. `pim` stores
lightweight profile manifests under `~/.pi-manager/profiles/<name>.json`.
When you run `pim <profile>`, it builds the profile's active view at
`~/.pi-manager/.active/<name>/` and **exec's into `pi`** with
`PI_CODING_AGENT_DIR` set to that directory.

This means:
- **Multiple profiles can run at the same time** in different terminals
- **No global state** — each pi instance has its own sessions, auth, configs
- **Never touches `~/.pi/agent`** — your default pi config is left alone
- **No more crashes** — switching profiles never affects running pi processes

### First-time migration

If you already have a real `~/.pi/agent/` directory from using pi normally,
the first time you run `pim use <name>`, it will automatically migrate
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

# Build/refresh active view and set as default
pim use work

# Set a default profile (launched when running `pim` with no args)
pim set-default work

# Show current status
pim status

# Delete a profile
pim delete experiments
pim delete experiments --force   # skip confirmation

# Migrate old-style profiles to the new JSON manifest format
pim migrate
```

### Quick reference

| Command | What it does |
|---------|-------------|
| `pim` | Launch pi with default profile |
| `pim research` | Launch pi with that profile |
| `pim -p "hello"` | Launch pi with default profile + args |
| `pim work -- -p "hi"` | Launch pi with profile + args |
| `pim use work` | Build active view + set as default |
| `pim edit work` | Edit profile selections interactively |
| `pim list` | List all profiles |
| `pim create work` | Create a new profile |
| `pim delete work` | Delete a profile |
| `pim status` | Show current status |
| `pim migrate` | Migrate old-style profiles |

## What a profile looks like

A profile is a lightweight JSON manifest under `~/.pi-manager/profiles/<name>.json`:

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

When launched, `pim` builds the profile's active view at `~/.pi-manager/.active/<name>/`
and exec's into `pi` with `PI_CODING_AGENT_DIR` set to that directory:

```
~/.pi-manager/.active/work/
├── settings.json
├── mcp.json
├── config/          ── symlinks → data/work/config/   (persistent config files)
├── extensions/      ── symlinks → pool/extensions/
├── skills/          ── symlinks → pool/skills/
├── prompts/         ── symlinks → pool/prompts/
├── auth.json        ── symlink  → data/work/auth.json
├── models.json      ── symlink  → data/work/models.json
├── trust.json       ── symlink  → data/work/trust.json
└── sessions/        ── symlinks → data/work/sessions/ (persistent sessions)
```

Key differences from the old design:
- **`~/.pi/agent` is never touched** — your default pi config is preserved
- **Active views are persistent** — never deleted automatically
- **Config files** are symlinked from `data/<name>/config/`, so runtime modifications persist
- **Session files** are symlinked from `data/<name>/sessions/`, so sessions survive rebuilds
- **Multiple profiles** can run simultaneously in different terminals

### How `PI_CODING_AGENT_DIR` works

Pi reads `PI_CODING_AGENT_DIR` on startup to locate its config directory.
pim sets this to the profile's active view before launching pi:

```bash
# What pim does internally:
PI_CODING_AGENT_DIR=~/.pi-manager/.active/research exec pi
```

You can also use this directly if you want to run pi with a specific
profile without pim:

```bash
PI_CODING_AGENT_DIR=~/.pi-manager/.active/research pi -p "hello"
```

### What happens to `~/.pi/agent`?

`~/.pi/agent` is **never touched by pim**. If you had a pre-existing
`~/.pi/agent` from using pi directly, it remains intact as your default
pi config. Running `pi` directly (without pim) will continue to use it.

If you have an old `~/.pi/agent` symlink from a previous version of pim,
it's harmless and can be removed:

```bash
rm ~/.pi/agent   # optional — pim no longer uses it
```

## Architecture

pim uses a **resource pool configuration model** — see [`docs/configuration.md`](docs/configuration.md) for the full design.

In short:

- **`pool/`** — global source of truth for extensions, skills, and prompts
- **`profiles/<name>.json`** — lightweight JSON manifests that select from the pool and declare configuration
- **`data/<name>/`** — auto-generated runtime state (auth tokens, sessions, config files)
- **`~/.pi/agent`** — **never touched by pim** — left as your default pi config

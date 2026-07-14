# pim

Profile manager for [pi](https://pi.dev). Switch between
independent sets of settings, extensions, skills, themes, and auth with a single
command.

## How it works

Pi reads its configuration from `~/.pi/agent/`. `pim` stores lightweight profile
manifests under `~/.pi-manager/profiles/<name>.json`. When you activate a profile,
`pim` dynamically constructs the profile's configuration (merging settings and pool resources)
under `~/.pi-manager/.active/<name>/` and sets `~/.pi/agent` as a **symlink** pointing to it.

You never wrap pi — just activate a profile with `pim`, then run `pi`
as usual. Pi reads `~/.pi/agent` naturally and gets the profile's config.

### First-time migration

If you already have a real `~/.pi/agent/` directory from using pi normally,
the first time you run `pim use <name>`, it will automatically migrate
your existing config into the new resource pool format and replace it with a symlink.

## Installation

```bash
cargo install --path /path/to/pim
```

Requires [Rust](https://rustup.rs/) and [pi](https://pi.dev)
(`npm install -g pi-coding-agent`).

## Usage

```bash
# Create a profile (empty, start fresh)
pim create work

# Create from your current ~/.pi/agent config
pim create work --from-base

# Copy from an existing profile
pim create experiments --from work

# Edit selections (extensions, skills, prompts) interactively
pim edit work

# List profiles (shows active ◀ and default markers)
pim list

# Set a default profile
pim set-default work

# Activate a profile (makes ~/.pi/agent point to its active view)
pim use work

# Then just run pi directly:
pi
pi -p "fix the bug"

# Activate the default profile
pim

# Show current status
pim status

# Delete a profile
pim delete experiments
pim delete experiments --force   # skip confirmation
```

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
  }
}
```

When activated, `pim` builds the effective active view at `~/.pi-manager/.active/<name>/`:

```
~/.pi-manager/.active/work/
├── settings.json
├── mcp.json
├── extensions/      ── symlinks → pool/extensions/
├── skills/          ── symlinks → pool/skills/
├── prompts/         ── symlinks → pool/prompts/
├── auth.json        ── symlink  → data/work/auth.json
└── sessions/        ── symlink  → data/work/sessions/
```

Since each profile links to its own data directory (`~/.pi-manager/data/<name>/`), you can log into
different accounts per profile (e.g., work GitHub vs personal GitHub).

## Architecture

pim uses a **resource pool configuration model** — see [`docs/configuration.md`](docs/configuration.md) for the full design.

In short:

- **`pool/`** — global source of truth for extensions, skills, and prompts
- **`profiles/<name>.json`** — lightweight JSON manifests that select from the pool and declare configuration
- **`data/<name>/`** — auto-generated runtime state (auth tokens, sessions)

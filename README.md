# pim

Profile manager for [pi](https://pi.dev). Switch between
independent sets of settings, extensions, skills, themes, and auth with a single
command.

## How it works

Pi reads its configuration from `~/.pi/agent/`. `pim` stores reusable
profile directories under `~/.pi-manager/profiles/<name>/` and sets `~/.pi/agent`
as a **symlink** pointing to the active profile.

You never wrap pi — just activate a profile with `pim`, then run `pi`
as usual. Pi reads `~/.pi/agent` naturally and gets the profile's config.

### First-time migration

If you already have a real `~/.pi/agent/` directory from using pi normally,
the first time you run `pim use <name>`, it will automatically migrate
your existing config into a pi-manager profile and replace it with a symlink.

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

# List profiles (shows active ◀ and default markers)
pim list

# Set a default profile
pim set-default work

# Activate a profile (makes ~/.pi/agent point to it)
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

Each profile is a complete pi `agentDir`:

```
~/.pi-manager/profiles/work/
├── settings.json
├── auth.json
├── models.json
├── extensions/
├── skills/
├── prompts/
└── sessions/
```

Since each profile gets its own `auth.json` and `models.json`, you can log into
different accounts per profile (e.g., work GitHub vs personal GitHub).

## How it's different from the old approach

Previously, `pim` launched pi directly by setting the
`PI_CODING_AGENT_DIR` environment variable. This meant you always had to type
`pim use <name>` to start coding.

Now `pim` is purely a config switcher — it manages `~/.pi/agent` as a
symlink. You activate once, then just run `pi` normally. The switch is
instant and doesn't interfere with pi's process or environment.
